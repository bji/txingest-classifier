use crate::config::Config;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::net::IpAddr;
use std::rc::Rc;

// xxx really want to evaluate in the time range leading up to and including leader slots.
//
// Threshold is:
// - Per second
// - Or 4x threshold over 6 seconds
// - Or 6x threshold over 12 seconds

// To track:

// Stake levels of peers
// failed + exceeded counts since last periodic
//    + deque with summary per second
// closed QUIC connections with no tx since last periodic
//    + deque with summary per second
//
//

#[derive(Default)]
pub struct State
{
    pub config : Config,

    // Timestamp of most recent event
    pub most_recent_timestamp : u64,

    // Number of events at the most recent timestamp
    pub most_recent_timestamp_event_count : u16,

    // Leader status -- how many slots until leader -- None until known
    pub leader_status : Option<LeaderStatus>,

    // Timestamps of failures
    pub failures : HashMap<IpAddr, VecDeque<u64>>,

    // Mapping from IP address to the Peer struct that records peer specific data
    pub peers : HashMap<IpAddr, Peer>,

    // Current tx.  Tracked for 5 minutes after last seen.
    pub current_tx : HashMap<Signature, Rc<RefCell<Tx>>>,

    // Timestamp of most recent 6 second period
    pub period_start : Option<u64>,

    // Total fee of most recent period
    pub recent_fees : Fee,

    // Average fee per tx + avg fee per cu per tx landed per 6 second interval for the previous 1 day
    // Timestamp on the fee is the
    pub avg_fees : VecDeque<TimestampedFee>,

    // Peers in the failed_exceeded_quic_threshold_set, map from IpAddr to timestamp of when put into set
    pub failed_exceeded_quic_threshold_set : HashMap<IpAddr, u64>,

    pub useless_quic_threshold_set : HashMap<IpAddr, u64>,

    // Peers in the threshold for "worst landed %"
    pub landed_pct_threshold_set : HashMap<IpAddr, u64>,

    // Peers in the threshold for "worst exclusive %"
    pub exclusive_pct_threshold_set : HashMap<IpAddr, u64>,

    // Peers in the threshold for "lowest fee per landed tx"
    pub fee_per_landed_tx_threshold_set : HashMap<IpAddr, u64>,

    // Peers in the threshold for "lowest fee per submitted tx"
    pub fee_per_submitted_tx_threshold_set : HashMap<IpAddr, u64>,

    // Peers in the threshold for "lowest fee/CU per landed tx"
    pub fee_per_cu_per_landed_tx_threshold_set : HashMap<IpAddr, u64>,

    // Peers in the threshold for "lowest fee/CU per submitted tx"
    pub fee_per_cu_per_submitted_tx_threshold_set : HashMap<IpAddr, u64>,

    // Peers in the number of slots before leader slots to apply the "outside leader slots" classifications.  If not
    // present, then leader slot based classification is not done
    pub leader_slot_classification_threshold_set : HashMap<IpAddr, u64>
}

#[derive(Default)]
pub struct Peer
{
    // Timestamp that an event was last seen from this peer
    pub timestamp : u64,

    // If the peer has an identity, this is it
    pub identity : Option<Pubkey>,

    // Stake level of peer
    pub stake : u64,

    // Total number of vote tx submitted
    pub vote_tx_submitted : u64,

    // Total number of user tx submitted
    pub user_tx_submitted : u64,

    // Recent QUIC connections.  This is the timestamp of the QUIC connection closing.
    pub connections : VecDeque<u64>,

    // Recent "useless QUIC connections", which are those which were closed by the remote peer and never submitted a
    // tx, or were closed by the local peer, lived at least 6 seconds, and never submitted a tx.  This is the
    // timestamp of the QUIC connection closing.
    pub useless_connections : VecDeque<u64>,

    // Tx submitted within the previous 6 seconds
    pub tx : VecDeque<SubmittedTx>
}

#[derive(Default)]
pub struct StakeLevel
{
    // Timestamp of most recent event seen for this peer
    pub timestamp : u64,

    // Stake level
    pub stake : u64
}

#[derive(Default)]
pub struct Tx
{
    // Timestamp that the tx was most recently seen
    pub timestamp : u64,

    // Set of peers who submitted this tx
    pub submitters : HashSet<IpAddr>,

    // Fee paid by the tx, if known
    pub fee : Option<Fee>
}

#[derive(Default)]
pub struct SubmittedTx
{
    pub timestamp : u64,

    pub tx : Rc<RefCell<Tx>>
}

#[derive(Default)]
pub struct Fee
{
    pub total : u64,

    pub cu_limit : u64,

    pub cu_used : u64
}

#[derive(Default)]
pub struct TimestampedFee
{
    pub timestamp : u64,

    pub fee : Fee
}

pub enum LeaderStatus
{
    // Not leader and not going to be leader soon
    NotSoon,

    // Upcoming in a given number of slots; the validator only reports upcoming within 200 slots of leader slots
    Upcoming(u64),

    // Now leader
    Leader
}

impl State
{
    pub fn new(config : Config) -> Self
    {
        Self { config, ..State::default() }
    }

    // Gets the timestamp to use given the reported timestamp of an event
    fn get_timestamp(
        &mut self,
        timestamp : u64
    ) -> u64
    {
        // If time stays the same or goes backwards, allow a maximum of 100 events before forcing time forward
        // by 1 ms.  This is to make the case where time goes backwards sane -- every 100 events will be considered
        // to be in the same millisecond, which is generally rational.
        if timestamp <= self.most_recent_timestamp {
            if self.most_recent_timestamp_event_count == 100 {
                self.most_recent_timestamp += 1;
                self.most_recent_timestamp_event_count = 0;
            }
        }
        else {
            self.most_recent_timestamp = timestamp;
            self.most_recent_timestamp_event_count = 0;
        }

        if self.period_start.is_none() {
            self.period_start = Some(self.most_recent_timestamp);
        }

        self.most_recent_timestamp
    }

    fn get_tx(
        &mut self,
        timestamp : u64,
        signature : Signature,
        submitter : IpAddr
    ) -> Rc<RefCell<Tx>>
    {
        self.current_tx
            .entry(signature)
            .and_modify(|tx| {
                tx.borrow_mut().timestamp = timestamp;
                let _ = tx.borrow_mut().submitters.insert(submitter);
            })
            .or_insert_with(|| Rc::new(RefCell::new(Tx { timestamp, submitters : [submitter].into(), fee : None })))
            .clone()
    }

    pub fn failed(
        &mut self,
        timestamp : u64,
        peer_addr : IpAddr
    )
    {
        let timestamp = self.get_timestamp(timestamp);

        self.failures.entry(peer_addr).or_default().push_back(timestamp);
    }

    pub fn exceeded(
        &mut self,
        timestamp : u64,
        peer_addr : IpAddr,
        peer_pubkey : Option<Pubkey>,
        stake : u64
    )
    {
        // Treat it as a failure by that IP address
        self.failed(timestamp, peer_addr.clone());

        // Additionally, record the identity and stake level if not previously known
        self.started(timestamp, peer_addr, peer_pubkey, stake);
    }

    pub fn started(
        &mut self,
        timestamp : u64,
        peer_addr : IpAddr,
        peer_pubkey : Option<Pubkey>,
        stake : u64
    )
    {
        let timestamp = self.get_timestamp(timestamp);

        let peer = self.peers.entry(peer_addr).or_default();

        peer.timestamp = timestamp;

        peer.identity = peer_pubkey;

        peer.stake = stake;
    }

    pub fn finished(
        &mut self,
        timestamp : u64,
        peer_addr : IpAddr
    )
    {
        let timestamp = self.get_timestamp(timestamp);

        if let Some(peer) = self.peers.get_mut(&peer_addr) {
            peer.connections.push_back(timestamp);
            if (peer.vote_tx_submitted == 0) && (peer.user_tx_submitted == 0) {
                peer.useless_connections.push_back(timestamp);
            }
        }
    }

    pub fn votetx(
        &mut self,
        timestamp : u64,
        peer_addr : IpAddr
    )
    {
        let timestamp = self.get_timestamp(timestamp);

        if let Some(peer) = self.peers.get_mut(&peer_addr) {
            peer.timestamp = timestamp;

            peer.vote_tx_submitted += 1;
        }
    }

    pub fn usertx(
        &mut self,
        timestamp : u64,
        peer_addr : IpAddr,
        signature : Signature
    )
    {
        let timestamp = self.get_timestamp(timestamp);

        let tx = self.get_tx(timestamp, signature, peer_addr.clone());

        if let Some(peer) = self.peers.get_mut(&peer_addr) {
            peer.timestamp = timestamp;

            peer.user_tx_submitted += 1;

            peer.tx.push_back(SubmittedTx { timestamp, tx });
        }
    }

    pub fn forwarded(
        &mut self,
        _timestamp : u64,
        _signature : Signature
    )
    {
        // Don't care
    }

    pub fn badfee(
        &mut self,
        _timestamp : u64,
        _signature : Signature
    )
    {
        // Don't care
    }

    pub fn fee(
        &mut self,
        timestamp : u64,
        signature : Signature,
        cu_limit : u64,
        cu_used : u64,
        fee : u64
    )
    {
        // Advance timestamp if necessary
        self.get_timestamp(timestamp);

        if let Some(tx) = self.current_tx.get_mut(&signature) {
            tx.borrow_mut().fee = Some(Fee { total : fee, cu_limit, cu_used });
        }

        self.recent_fees.total += fee;
        self.recent_fees.cu_limit += cu_limit;
        self.recent_fees.cu_used += cu_used;
    }

    pub fn will_be_leader(
        &mut self,
        timestamp : u64,
        slots : u8
    )
    {
        self.get_timestamp(timestamp);

        self.leader_status = Some(LeaderStatus::Upcoming(slots as u64));

        if let Some(leader_slot_classification_threshold) = self.config.leader_slot_classification_threshold {
            if (slots as u64) > leader_slot_classification_threshold {
                println!("NOT LEADER CLASSIFICATION");
            }
            else {
                println!("LEADER CLASSIFICATION");
            }
        }
    }

    pub fn begin_leader(
        &mut self,
        timestamp : u64
    )
    {
        self.get_timestamp(timestamp);

        self.leader_status = Some(LeaderStatus::Leader);

        if self.config.leader_slot_classification_threshold.is_some() {
            println!("LEADER CLASSIFICATION");
        }
    }

    pub fn end_leader(
        &mut self,
        timestamp : u64
    )
    {
        self.get_timestamp(timestamp);

        self.leader_status = Some(LeaderStatus::NotSoon);

        if self.config.leader_slot_classification_threshold.is_some() {
            println!("NOT LEADER CLASSIFICATION");
        }
    }

    // Do periodic work: log stuff and clean.  Would be better to do it all based on timers instead of periodic
    // polling but this code isn't that sophisticated yet.  Call once per second.
    pub fn periodic(
        &mut self,
        now : u64
    )
    {
        let five_minutes_ago = now - (5 * 60 * 1000);

        // If it's time for a new period, then use recent_fees to produce a new avg_fees
        if let Some(period_start) = self.period_start {
            let next_period_start = period_start + (6 * 1000);
            if now < next_period_start {
                // If the current period has not completed, nothing more to do in this function
                return;
            }
            let duration = (now - period_start) / 1000;
            self.avg_fees.push_back(TimestampedFee {
                timestamp : now,
                fee : Fee {
                    total : self.recent_fees.total / duration,
                    cu_limit : self.recent_fees.cu_limit / duration,
                    cu_used : self.recent_fees.cu_used / duration
                }
            });
            // Only allow as many 6 second periods as will fit into 24 hours
            while self.avg_fees.len() > ((24 * 60 * 60) / 6) {
                self.avg_fees.pop_front();
            }
            self.recent_fees = Fee::default();
            self.period_start = Some(now);
        }
        else {
            // If no current period has started, nothing more to do in this function
            return;
        }

        // Getting to this point means that a period has just completed, so re-evaluate all sets
        let avg_fees_seconds = (self.avg_fees.len() as u64) * 6;

        // Compute average fee over the previous 1 day
        let (avg_fee, avg_cu_limit, avg_cu_used) = if avg_fees_seconds > 0 {
            let mut total_fee = 0_u64;
            let mut total_cu_limit = 0_u64;
            let mut total_cu_used = 0_u64;

            for fee in &self.avg_fees {
                total_fee += fee.fee.total;
                total_cu_limit += fee.fee.cu_limit;
                total_cu_used += fee.fee.cu_used;
            }
            (total_fee / avg_fees_seconds, total_cu_limit / avg_fees_seconds, total_cu_used / avg_fees_seconds)
        }
        else {
            (0, 0, 0)
        };

        println!("Avg Fee: {avg_fee}");
        println!("Avg CU Limit: {avg_cu_limit}");
        println!("Avg CU Used: {avg_cu_used}");
        println!("Avg Fee/CU Limit: {:0.9}", (avg_fee as f64) / (avg_cu_limit as f64));
        println!("Avg Fee/CU Used: {:0.9}", (avg_fee as f64) / (avg_cu_used as f64));

        // Process peers - remove connection timestamps for connections which finished more than 1 day ago
        let one_day_ago = now - (24 * 60 * 60 * 1000);

        for (ip_addr, peer) in &mut self.peers {
            // Remove old connections and useless_connections
            loop {
                if let Some(front) = peer.connections.front() {
                    if *front < one_day_ago {
                        peer.connections.pop_front();
                    }
                    else {
                        break;
                    }
                }
                else {
                    break;
                }
            }
            loop {
                if let Some(front) = peer.useless_connections.front() {
                    if *front < one_day_ago {
                        peer.useless_connections.pop_front();
                    }
                    else {
                        break;
                    }
                }
                else {
                    break;
                }
            }
            // Compute percent useless connections over the previous 1 day
            if peer.connections.len() > 0 {
                let percent_useless = (peer.useless_connections.len() as f64) / (peer.connections.len() as f64);
                println!("{ip_addr} useless percent: {:0.3}", percent_useless * 100.0);
            }
            else {
                println!("{ip_addr} useless percent: N/A");
            }
        }

        // Remove tx older than 5 minutes old
        self.current_tx.retain(|_, tx| tx.borrow().timestamp >= five_minutes_ago);
    }
}
