use crate::classification::Classification;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct PubkeyClassification
{
    // If not provided, default of "known_pubkeys" is used
    pub group_name : Option<String>,

    // How long ip addresses that exceeded the threshold are held in the group before being expired out.  If not
    // specified, a default value of 24 hours is used.  Thresholds may provide their own value.
    pub group_expiration_seconds : Option<u64>,

    pub pubkey : String
}

#[derive(Deserialize)]
pub struct LeaderSlotsClassification
{
    pub group_name : Option<String>,

    pub leader_slots : u64
}

#[derive(Deserialize)]
pub struct Config
{
    // Classification by known pubkey
    pub known_pubkeys : Option<Vec<PubkeyClassification>>,

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
        if let Some(failed_exceeded_quic_connections) = &mut self.failed_exceeded_quic_connections {
            failed_exceeded_quic_connections.validate("failed_exceeded_quic_connections")?;
        }

        if self.useless_quic_connection_duration_ms.unwrap_or(1) == 0 {
            return Err("Invalid zero useless_quic_connection_duration_ms in config".to_string());
        }

        if let Some(useless_quic_connections) = &mut self.useless_quic_connections {
            useless_quic_connections.validate("useless_quic_connections")?;
        }

        if let Some(fee_lamports_submitted) = &mut self.fee_lamports_submitted {
            fee_lamports_submitted.validate("fee_lamports_submitted")?;
        }

        if let Some(fee_microlamports_per_cu_limit) = &mut self.fee_microlamports_per_cu_limit {
            fee_microlamports_per_cu_limit.validate("fee_microlamports_per_cu_limit")?;
        }

        if let Some(fee_microlamports_per_cu_used) = &mut self.fee_microlamports_per_cu_used {
            fee_microlamports_per_cu_used.validate("fee_microlamports_per_cu_used")?;
        }

        if let Some(outside_leader_slots) = &mut self.outside_leader_slots {
            outside_leader_slots.validate()?;
        }

        Ok(())
    }
}

// Must be called immediately after deserialization.  Validates that the LeaderSlotsClassification has rational values.
impl LeaderSlotsClassification
{
    pub fn validate(&mut self) -> Result<(), String>
    {
        if self.group_name.is_none() {
            self.group_name = Some("outside_leader_slots".to_string());
        }

        if self.group_name.as_ref().unwrap() == "" {
            return Err("Invalid outside_leader_slots group name: empty string".to_string());
        }

        if self.leader_slots > 432000 {
            return Err("Invalid outside_leader_slots leader_slots; must be <= 432000".to_string());
        }

        Ok(())
    }
}
