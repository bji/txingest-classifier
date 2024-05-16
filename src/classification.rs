use crate::group::{Group, DEFAULT_GROUP_EXPIRATION_SECONDS};
use crate::threshold::Threshold;
use serde::Deserialize;
use std::collections::{HashMap, VecDeque};
use std::net::IpAddr;

#[derive(Deserialize)]
pub struct Classification
{
    // group_name to use for any contained Threshold which does not supply its own group_name; defaults to the
    // name of the classification if not present here
    pub group_name : Option<String>,

    // How long ip addresses that exceeded the threshold are held in the group before being expired out.  If not
    // specified, a default value of 24 hours is used.  Thresholds may provide their own value.
    pub group_expiration_seconds : Option<u64>,

    // If this is present and true, then all thresholds will be evaluated no matter how many match.  If not present or
    // false, then the first threshold which matches will stop evaluation of subsequent thresholds
    pub evaluate_all_thresholds : Option<bool>,

    // The thresholds to apply
    pub thresholds : Vec<Threshold>,

    #[serde(skip)]
    max_duration_ms : u64,

    #[serde(skip)]
    recent_values : HashMap<IpAddr, VecDeque<TimestampedValue>>
}

pub struct TimestampedValue
{
    pub timestamp : u64,

    pub value : u64
}

// Created by deserialization from config file.
impl Classification
{
    // Must be called immediately after deserialization.  Validates that the Classification has rational values.
    pub fn validate(
        &mut self,
        name : &str
    ) -> Result<(), String>
    {
        if self.thresholds.is_empty() {
            return Err(format!("Classification {name} has no thresholds"));
        }

        for index in 0..self.thresholds.len() {
            let threshold = &mut self.thresholds[index];
            threshold.validate(
                name,
                index,
                self.group_name.as_ref().map(|str| str.as_str()).unwrap_or(name),
                self.group_expiration_seconds.unwrap_or(DEFAULT_GROUP_EXPIRATION_SECONDS) * 1000
            )?;
            if threshold.duration_ms > self.max_duration_ms {
                self.max_duration_ms = threshold.duration_ms;
            }
        }

        Ok(())
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
        groups : &mut HashMap<String, Group>,
        now : u64
    )
    {
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

        // Call threshold periodic for each ip address, stopping if a threshold has been met for that address and
        // the classification calls for stopping after the first matching threshold for an ip address
        for (ip_addr, recent_values) in &self.recent_values {
            for threshold in &mut self.thresholds {
                if threshold.is_exceeded(stakes, now, ip_addr, recent_values, groups) &&
                    !self.evaluate_all_thresholds.unwrap_or(false)
                {
                    break;
                }
            }
        }
    }
}
