use crate::classification::TimestampedValue;
use crate::group::Group;
use serde::Deserialize;
use std::collections::{HashMap, VecDeque};
use std::net::IpAddr;

#[derive(Deserialize)]
pub struct Threshold
{
    // Group name to add peers who exceed this threshold to; if not provided, defaults to the group name specified
    // in the containing classification
    pub group_name : Option<String>,

    // How long ip addresses that exceeded the threshold are held in the group before being expired out.  If not
    // specified, a default value of 24 hours is used.
    pub group_expiration_seconds : Option<u64>,

    // If present, this threshold will only apply to ip addresses of staked validators with stake >= this value
    pub low_stake : Option<u64>,

    // If present, this threshold will only apply to ip addresses of staked validators with stake <= this value
    pub high_stake : Option<u64>,

    // Comparison operation to use when comparing accumulated values for an ip address with the threshold value to
    // determine if the ip address has met the threshold and thus should be included in the group
    pub threshold_type : ThresholdType,

    // Minimum number of events before the threshold is applied
    pub min_value_count : Option<u64>,

    // The value to compare accumulated values to
    pub value : u64,

    // The time span in milliseconds over which to sum accumulated values to get the value to compare against
    pub duration_ms : u64
}

#[derive(Deserialize)]
pub enum ThresholdType
{
    // If the value is greater than the threshold, then it meets the classification criteria
    #[serde(rename = "greater_than")]
    GreaterThan,

    // If the value is greater than or equal to the threshold, then it meets the classification criteria
    #[serde(rename = "greater_than_or_equal_to")]
    GreaterThanOrEqual,

    // If the value is lower than the threshold, then it meets the classification criteria
    #[serde(rename = "less_than")]
    LessThan,

    // If the value is lower than or equal to the threshold, then it meets the classification criteria
    #[serde(rename = "less_than_or_equal_to")]
    LessThanOrEqual
}

impl Threshold
{
    pub fn validate(
        &mut self,
        classification_name : &str,
        threshold_index : usize,
        classification_group_name : &str,
        classification_group_expiration_seconds : u64
    ) -> Result<(), String>
    {
        if let Some(low_stake) = self.low_stake {
            if let Some(high_stake) = self.high_stake {
                if high_stake < low_stake {
                    return Err(format!(
                        "Classification {classification_name} has threshold at index {threshold_index} that invalidly \
                         specifies high_stake {high_stake} as lower than low_stake {low_stake}"
                    ));
                }
            }
        }

        if self.duration_ms == 0 {
            return Err(format!(
                "Classification {classification_name} has threshold at index {threshold_index} with zero duration_ms"
            ));
        }

        if self.group_name.is_none() {
            self.group_name = Some(classification_group_name.to_string());
        }

        if self.group_expiration_seconds.is_none() {
            self.group_expiration_seconds = Some(classification_group_expiration_seconds);
        }

        Ok(())
    }

    pub fn is_exceeded(
        &mut self,
        stakes : &HashMap<IpAddr, u64>,
        now : u64,
        ip_addr : &IpAddr,
        recent_values : &VecDeque<TimestampedValue>,
        groups : &mut HashMap<String, Group>
    ) -> bool
    {
        // Skip this threshold check if the stake level of the ip_addr doesn't match
        if let Some(low_stake) = self.low_stake {
            let stake = *(stakes.get(ip_addr).unwrap_or(&0));
            if stake < low_stake {
                return false;
            }
            if let Some(high_stake) = self.high_stake {
                if stake > high_stake {
                    return false;
                }
            }
        }
        else if let Some(high_stake) = self.high_stake {
            if *(stakes.get(ip_addr).unwrap_or(&0)) > high_stake {
                return false;
            }
        }

        let use_timestamp = now - self.duration_ms;

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

        if let Some(min_value_count) = self.min_value_count {
            if value_count < min_value_count {
                return false;
            }
        }

        let is_in_group = match self.threshold_type {
            ThresholdType::GreaterThan => value_sum > self.value,
            ThresholdType::GreaterThanOrEqual => value_sum >= self.value,
            ThresholdType::LessThan => value_sum < self.value,
            ThresholdType::LessThanOrEqual => value_sum <= self.value
        };

        if is_in_group {
            let group_name = self.group_name.as_ref().unwrap();
            groups
                .entry(group_name.clone())
                .or_insert_with(|| Group::new(group_name))
                .add(ip_addr.clone(), now + self.group_expiration_seconds.unwrap());
            true
        }
        else {
            false
        }
    }
}
