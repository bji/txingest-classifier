use serde::Deserialize;

const DEFAULT_LOW_STAKE_SOL : u64 = 1000;

#[derive(Deserialize)]
pub enum Threshold<T>
{
    // Threshold by value above or below mean value: values above or below the mean (depending on what is being
    // classified) by the specific given value are in the class
    OutsideMean(T),

    // Threshold by absolute value: values above or below (depending on what is being classified) the specific given
    // value are in the class
    Value(T)
}

#[derive(Default, Deserialize)]
pub struct Config
{
    // The threshold SOL below which a gossip peer is considered low stake
    pub low_stake_sol : Option<u64>,

    // The threshold for "worst failed/exceeded QUIC connections", in connections per second
    pub failed_exceeded_quic_threshold : Option<Threshold<u64>>,

    // The threshold for "worst useless QUIC connections", in connections per second
    pub useless_quic_threshold : Option<Threshold<u64>>,

    // The threshold for "worst landed %"
    pub landed_pct_threshold : Option<Threshold<f64>>,

    // The threshold for "worst exclusive %"
    pub exclusive_pct_threshold : Option<Threshold<f64>>,

    // The threshold for "lowest fee per landed tx"
    pub fee_per_landed_tx_threshold : Option<Threshold<u64>>,

    // The threshold for "lowest fee per submitted tx"
    pub fee_per_submitted_tx_threshold : Option<Threshold<u64>>,

    // The threshold for "lowest fee/CU per landed tx"
    pub fee_per_cu_per_landed_tx_threshold : Option<Threshold<u64>>,

    // The threshold for "lowest fee/CU per submitted tx"
    pub fee_per_cu_per_submitted_tx_threshold : Option<Threshold<u64>>,

    // Number of slots before leader slots to apply the "outside leader slots" classifications.  If not present,
    // then leader slot based classification is not done
    pub leader_slot_classification_threshold : Option<u64>
}
