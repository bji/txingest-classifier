use crate::config::{Classification, ThresholdType};
use std::collections::{HashMap, VecDeque};
use std::net::IpAddr;

const DEFAULT_GROUP_EXPIRATION_SECONDS : u64 = 24 * 60 * 60; // One day

// A Group manages the membership of ip addresses in a classifier group.
pub struct Group
{
    classification : Classification,

    max_duration_ms : u64,

    recent_values : HashMap<IpAddr, VecDeque<TimestampedValue>>,

    // Map from member to timestamp of when the member was added
    members : HashMap<IpAddr, u64>
}

struct TimestampedValue
{
    pub timestamp : u64,

    pub value : u64
}

impl Group
{
    pub fn new_option(classification : Option<Classification>) -> Option<Self>
    {
        classification.map(|classification| {
            let max_duration_ms =
                classification.thresholds.iter().map(|threshold| threshold.duration_ms).max().clone().unwrap();

            Self { classification, max_duration_ms, recent_values : Default::default(), members : Default::default() }
        })
    }

    pub fn add_value(
        &mut self,
        ip_addr : IpAddr,
        timestamp : u64,
        value : u64
    )
    {
        self.recent_values.entry(ip_addr).or_default().push_back(TimestampedValue { timestamp, value });
    }

    // To be called once per second
    pub fn periodic(
        &mut self,
        stakes : &HashMap<IpAddr, u64>,
        now : u64
    )
    {
        // Expire group memberships that are too old
        let retain_timestamp =
            now - (self.classification.group_expiration_seconds.unwrap_or(DEFAULT_GROUP_EXPIRATION_SECONDS) * 1000);
        self.members.retain(|ip_addr, added_timestamp| {
            if *added_timestamp < retain_timestamp {
                println!("Remove {ip_addr} from group {}", self.classification.group_name);
                false
            }
            else {
                true
            }
        });

        let retain_timestamp = now - self.max_duration_ms;

        // Clear out values that are too old
        for recent_values in self.recent_values.values_mut() {
            loop {
                if let Some(front) = recent_values.front() {
                    if front.timestamp < retain_timestamp {
                        recent_values.pop_front();
                    }
                    else {
                        break;
                    }
                }
                else {
                    break;
                }
            }
        }
        self.recent_values.retain(|_, recent_values| !recent_values.is_empty());

        // Apply thresholds
        for (ip_addr, recent_values) in &self.recent_values {
            for threshold in &self.classification.thresholds {
                // Skip this threshold check if the stake level of the ip_addr doesn't match
                if let Some(low_stake) = threshold.low_stake {
                    let stake = stakes.get(ip_addr).cloned().unwrap_or(0);
                    if stake < low_stake {
                        continue;
                    }
                    if let Some(high_stake) = threshold.high_stake {
                        if stake > high_stake {
                            continue;
                        }
                    }
                }
                else if let Some(high_stake) = threshold.high_stake {
                    if stakes.get(ip_addr).cloned().unwrap_or(0) > high_stake {
                        continue;
                    }
                }

                let use_timestamp = now - threshold.duration_ms;

                // Sum values for relevant timestamps
                let mut value_count = 0;
                let value_sum = recent_values
                    .iter()
                    .filter_map(|timestamped_value| {
                        if timestamped_value.timestamp < use_timestamp {
                            None
                        }
                        else {
                            value_count += 1;
                            Some(timestamped_value.value)
                        }
                    })
                    .sum::<u64>();

                if let Some(min_value_count) = threshold.min_value_count {
                    if value_count < min_value_count {
                        continue;
                    }
                }

                let is_in_group = match self.classification.threshold_type {
                    ThresholdType::GreaterThan => value_sum > threshold.value,
                    ThresholdType::GreaterThanOrEqual => value_sum >= threshold.value,
                    ThresholdType::LessThan => value_sum < threshold.value,
                    ThresholdType::LessThanOrEqual => value_sum <= threshold.value
                };

                if is_in_group && !self.members.contains_key(ip_addr) {
                    // Add ip_addr to the group
                    self.members.insert(ip_addr.clone(), now);
                    // Add ip_addr to the iptables group
                    println!("Add {ip_addr} to group {}", self.classification.group_name);
                    break;
                }
            }
        }
    }
}
