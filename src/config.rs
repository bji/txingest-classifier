use serde::Deserialize;

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

#[derive(Deserialize)]
pub struct Threshold
{
    pub low_stake : Option<u64>,

    pub high_stake : Option<u64>,

    // Minimum number of events before the threshold is applied
    pub min_value_count : Option<u64>,

    pub value : u64,

    pub duration_ms : u64
}

#[derive(Deserialize)]
pub struct Classification
{
    pub group_name : String,

    // How long ip addresses are held in the group before being expired out.  If not specified, a default value
    // of 24 hours is used.
    pub group_expiration_seconds : Option<u64>,

    // Any ip address which has not received any values in this number of seconds, is removed from the classifier
    // (but not the group).  If not specified, a default value of 24 hours is used.
    pub classification_expiration_seconds : Option<u64>,

    pub threshold_type : ThresholdType,

    pub thresholds : Vec<Threshold>
}

#[derive(Deserialize)]
pub struct LeaderSlotsClassification
{
    pub group_name : String,

    pub leader_slots : u64
}

#[derive(Deserialize)]
pub struct Config
{
    pub failed_exceeded_quic_connections : Option<Classification>,

    // Threshold in number of milliseconds for a QUIC connection which submitted no tx before being closed, for
    // the connection to be considered "useless".  If not present, a default of 2 seconds is used.
    pub useless_quic_connection_duration_ms : Option<u64>,

    pub useless_quic_connections : Option<Classification>,

    // Only the first submitter of a tx that is submitted by multiple sources gets fee credit for the tx.

    // fees are only added to the following classifications 5 minutes after the first submission of the tx.
    // The source that first submitted the tx is credited with the full fee.  All other submitters are
    // credited with 0 fee.

    // Lamports paid of submitted tx.  Every tx submitted gets a value; for tx which never landed, the fee will be
    // given as value 0.
    pub fee_lamports_submitted : Option<Classification>,

    // Microlamports per CU for each tx landed, where the CU value is that declared as the CU limit by the tx.
    // Note that only landed tx are included here (xxx this can be improved by parsing tx contents at tx ingestion
    // time).
    pub fee_microlamports_per_cu_limit : Option<Classification>,

    // Microlamports per CU for each tx landed, where the CU value is the CU actually used during execution of the tx.
    // Note that only landed tx are included here.
    pub fee_microlamports_per_cu_used : Option<Classification>,

    // Number of slots before leader slots to apply the "outside leader slots" classifications.  If not present, then
    // this categorization is not performed.
    pub outside_leader_slots : Option<LeaderSlotsClassification>
}

// Must be called immediately after deserialization.  Validates that the Config has rational values.
impl Config
{
    pub fn validate(&mut self) -> Result<(), String>
    {
        if let Some(failed_exceeded_quic_connections) = &self.failed_exceeded_quic_connections {
            failed_exceeded_quic_connections.validate()?;
        }

        if let Some(useless_quic_connections) = &self.useless_quic_connections {
            useless_quic_connections.validate()?;
        }

        if let Some(fee_lamports_submitted) = &self.fee_lamports_submitted {
            fee_lamports_submitted.validate()?;
        }

        if let Some(fee_microlamports_per_cu_limit) = &self.fee_microlamports_per_cu_limit {
            fee_microlamports_per_cu_limit.validate()?;
        }

        if let Some(fee_microlamports_per_cu_used) = &self.fee_microlamports_per_cu_used {
            fee_microlamports_per_cu_used.validate()?;
        }

        if let Some(outside_leader_slots) = &self.outside_leader_slots {
            outside_leader_slots.validate()?;
        }

        Ok(())
    }
}

// Created by deserialization from config file.
impl Classification
{
    // Must be called immediately after deserialization.  Validates that the Classification has rational values.
    pub fn validate(&self) -> Result<(), String>
    {
        if self.group_name == "" {
            return Err("Invalid classification group name: empty string".to_string());
        }

        if let Some(group_expiration_seconds) = self.group_expiration_seconds {
            if group_expiration_seconds == 0 {
                return Err(format!("Invalid classification 0 expiration seconds for group {}", self.group_name));
            }
        }

        if self.thresholds.is_empty() {
            return Err(format!("Classification for group \"{}\" has no thresholds", self.group_name));
        }

        for index in 0..self.thresholds.len() {
            let threshold = &self.thresholds[index];
            if let Some(low_stake) = threshold.low_stake {
                if let Some(high_stake) = threshold.high_stake {
                    if high_stake < low_stake {
                        return Err(format!(
                            "Classification for group \"{}\" has threshold at index {index} that invalidly specifies \
                             high_stake {high_stake} as lower than low_stake {low_stake}",
                            self.group_name
                        ));
                    }
                }
            }

            if threshold.duration_ms == 0 {
                return Err(format!(
                    "Classification for group \"{}\" has threshold at index {index} with 0 duration_ms",
                    self.group_name
                ));
            }
        }

        Ok(())
    }
}

// Must be called immediately after deserialization.  Validates that the LeaderSlotsClassification has rational values.
impl LeaderSlotsClassification
{
    pub fn validate(&self) -> Result<(), String>
    {
        if self.group_name == "" {
            return Err("Invalid outside_leader_slots group name: empty string".to_string());
        }

        if self.leader_slots > 432000 {
            return Err("Invalid outside_leader_slots leader_slots; must be <= 432000".to_string());
        }

        Ok(())
    }
}
