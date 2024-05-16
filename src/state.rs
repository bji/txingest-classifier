use crate::{
    config::{Config, LeaderSlotsClassification},
    group::Group
};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use std::collections::{HashMap, HashSet};
use std::net::IpAddr;

const DEFAULT_USELESS_QUIC_CONNECTION_DURATION_MS : u64 = 2 * 1000; // 2 seconds
const TX_RETENTION_DURATION_MS : u64 = 2 * 60 * 1000; // 2 minutes
const PEER_RETENTION_DURATION_MS : u64 = 3 * 24 * 60 * 60 * 1000; // 3 days

pub struct State
{
    // Fee that represents a tx that paid no fee
    pub zero_fee : Fee,

    pub useless_quic_connection_duration_ms : u64,

    // Timestamp of most recent event
    pub most_recent_timestamp : u64,

    // Number of events at the most recent timestamp
    pub most_recent_timestamp_event_count : u16,

    // Leader status -- Some(true) if leader, Some(false) if not; None until known
    pub leader_status : Option<bool>,

    // Mapping from IP address to the Peer struct that records peer specific data
    pub peers : HashMap<IpAddr, Peer>,

    // Peer stake information
    pub stakes : HashMap<IpAddr, u64>,

    pub failed_exceeded_quic_connections_group : Option<Group>,

    pub useless_quic_connections_group : Option<Group>,

    pub fee_lamports_submitted_group : Option<Group>,

    // Not supported yet -- need to parse the cu limit out for all submitted tx in order to track this
    // pub fee_microlamports_per_cu_limit_submitted_group : Option<Group>,
    pub fee_microlamports_per_cu_limit_landed_group : Option<Group>,

    pub fee_microlamports_per_cu_used_landed_group : Option<Group>,

    pub outside_leader_slots : Option<LeaderSlotsClassification>,

    // Current tx.  Tracked for 5 minutes after first seen.
    pub current_tx : HashMap<Signature, Tx>
}

#[derive(Default)]
pub struct Peer
{
    // Timestamp of first event seen from this peer
    pub first_timestamp : u64,

    // Timestamp that an event was last seen from this peer
    pub most_recent_timestamp : u64,

    // If the peer has an identity, this is it
    pub identity : Option<Pubkey>,

    // Total number of tx submitted (votes + user)
    pub tx_submitted : u64
}

#[derive(Default)]
pub struct Tx
{
    // Submitters
    pub submitters : HashSet<IpAddr>,

    // Submitters of the tx, in order of first submission
    pub submissions : Vec<SubmittedTx>,

    // Fee paid by the tx, if known
    pub fee : Option<Fee>
}

pub struct SubmittedTx
{
    pub timestamp : u64,

    pub submitter : IpAddr
}

impl Tx
{
    pub fn new(
        timestamp : u64,
        first_submitter : IpAddr
    ) -> Self
    {
        Self {
            submitters : vec![first_submitter].into_iter().collect(),
            submissions : vec![SubmittedTx { timestamp, submitter : first_submitter.clone() }],
            fee : None
        }
    }

    pub fn submitted(
        &mut self,
        timestamp : u64,
        submitter : IpAddr
    )
    {
        // If it's already been submitted by this submitter, then nothing more to do
        if self.submitters.contains(&submitter) {
            return;
        }

        self.submitters.insert(submitter);

        self.submissions.push(SubmittedTx { timestamp, submitter : submitter.clone() });
    }
}

#[derive(Default)]
pub struct Fee
{
    pub total : u64,

    pub cu_limit : u64,

    pub cu_used : u64
}

impl State
{
    pub fn new(config : Config) -> Self
    {
        Self {
            zero_fee : Fee { total : 0, cu_limit : 1, cu_used : 1 },
            useless_quic_connection_duration_ms : config
                .useless_quic_connection_duration_ms
                .unwrap_or(DEFAULT_USELESS_QUIC_CONNECTION_DURATION_MS),
            most_recent_timestamp : 0,
            most_recent_timestamp_event_count : 0,
            leader_status : None,
            peers : Default::default(),
            stakes : Default::default(),
            failed_exceeded_quic_connections_group : Group::new_option(config.failed_exceeded_quic_connections),
            useless_quic_connections_group : Group::new_option(config.useless_quic_connections),
            fee_lamports_submitted_group : Group::new_option(config.fee_lamports_submitted),
            fee_microlamports_per_cu_limit_landed_group : Group::new_option(config.fee_microlamports_per_cu_limit),
            fee_microlamports_per_cu_used_landed_group : Group::new_option(config.fee_microlamports_per_cu_used),
            outside_leader_slots : config.outside_leader_slots,
            current_tx : Default::default()
        }
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

        self.most_recent_timestamp
    }

    pub fn failed(
        &mut self,
        timestamp : u64,
        peer_addr : IpAddr
    )
    {
        let timestamp = self.get_timestamp(timestamp);

        if let Some(failed_exceeded_quic_connections_group) = &mut self.failed_exceeded_quic_connections_group {
            failed_exceeded_quic_connections_group.add_value(peer_addr, timestamp, 1);
        }
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

        let peer = self.peers.entry(peer_addr.clone()).or_insert_with(|| Peer {
            first_timestamp : timestamp,
            most_recent_timestamp : timestamp,
            ..Peer::default()
        });

        peer.most_recent_timestamp = timestamp;

        peer.identity = peer_pubkey;

        self.stakes.insert(peer_addr, stake);
    }

    pub fn finished(
        &mut self,
        timestamp : u64,
        peer_addr : IpAddr
    )
    {
        let timestamp = self.get_timestamp(timestamp);

        if let Some(peer) = self.peers.get_mut(&peer_addr) {
            peer.most_recent_timestamp = timestamp;

            if let Some(useless_quic_connections_group) = &mut self.useless_quic_connections_group {
                if (peer.tx_submitted == 0) &&
                    ((timestamp - peer.first_timestamp) >= self.useless_quic_connection_duration_ms)
                {
                    useless_quic_connections_group.add_value(peer_addr, timestamp, 1);
                }
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
            peer.most_recent_timestamp = timestamp;

            peer.tx_submitted += 1;
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

        if let Some(peer) = self.peers.get_mut(&peer_addr) {
            peer.most_recent_timestamp = timestamp;

            peer.tx_submitted += 1;
        }

        // Only if this is the first time this peer has submitted this tx should the submitter be added to the
        // submissions list; all other submissions by the same peer are just re-submissions and are not accounted for,
        // so as not to count every one as a no-fee submitted tx which would lower the average tx fee rate for the
        // submitter
        self.current_tx
            .entry(signature)
            .and_modify(|tx| tx.submitted(timestamp, peer_addr))
            .or_insert_with(|| Tx::new(timestamp, peer_addr));
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
            tx.fee = Some(Fee { total : fee, cu_limit, cu_used });
        }
    }

    pub fn will_be_leader(
        &mut self,
        timestamp : u64,
        slots : u8
    )
    {
        if let Some(outside_leader_slots) = &mut self.outside_leader_slots {
            if (slots as u64) >= outside_leader_slots.leader_slots {
                self.end_leader(timestamp);
                return;
            }
        }
        // If leader slots aren't being tracked, then use begin_leader to ensure that peers are treated as if we're
        // leader and not blocked just because we're outside of leader slots

        self.begin_leader(timestamp);
    }

    pub fn begin_leader(
        &mut self,
        _timestamp : u64
    )
    {
        if !self.outside_leader_slots.is_some() || !self.leader_status.unwrap_or(false) {
            println!("LEADER CLASSIFICATION");
            self.leader_status = Some(true);
        }
    }

    pub fn end_leader(
        &mut self,
        timestamp : u64
    )
    {
        if self.outside_leader_slots.is_some() {
            if self.leader_status.unwrap_or(true) {
                // If currently in leader state
                println!("NOT LEADER CLASSIFICATION");
                self.leader_status = Some(false);
            }
        }
        // If leader slots aren't being tracked, then use begin_leader to ensure that peers are treated as if we're
        // leader and not blocked just because we're outside of leader slots
        else {
            self.begin_leader(timestamp);
        }
    }

    // Do periodic work: log stuff and clean.  Would be better to do it all based on timers instead of periodic
    // polling but this code isn't that sophisticated yet.  Call once per second.
    pub fn periodic(
        &mut self,
        now : u64
    )
    {
        // Convert now into a timestamp
        let now = self.get_timestamp(now);

        // If the leader_status classification has not happened yet, then we've just started up and haven't been
        // told anything about leader slots, so should assume we're outside of leader slots
        if self.leader_status.is_none() {
            self.end_leader(now);
        }

        //        // If it's time for a new period, then use recent_fees to produce a new avg_fees
        //        if let Some(period_start) = self.period_start {
        //            let next_period_start = period_start + PERIOD_DURATION_MS;
        //            if now < next_period_start {
        //                // If the current period has not completed, nothing more to do in this function
        //                return;
        //            }
        //            let duration = (now - period_start) / 1000;
        //            self.avg_fees.push_back(TimestampedFee {
        //                timestamp : now,
        //                fee : Fee {
        //                    total : self.recent_fees.total / duration,
        //                    cu_limit : self.recent_fees.cu_limit / duration,
        //                    cu_used : self.recent_fees.cu_used / duration
        //                }
        //            });
        //            // Only allow as many 6 second periods as will fit into 24 hours
        //            while self.avg_fees.len() > ((24 * 60 * 60) / 6) {
        //                self.avg_fees.pop_front();
        //            }
        //            self.recent_fees = Fee::default();
        //            self.period_start = Some(now);
        //        }
        //        else {
        //            // If no current period has started, nothing more to do in this function
        //            return;
        //        }
        //
        //        // Getting to this point means that a period has just completed, so re-evaluate all sets
        //        let avg_fees_seconds = (self.avg_fees.len() as u64) * 6;
        //
        //        // Compute average fee over the previous 1 day
        //        let (avg_fee, avg_cu_limit, avg_cu_used) = if avg_fees_seconds > 0 {
        //            let mut total_fee = 0_u64;
        //            let mut total_cu_limit = 0_u64;
        //            let mut total_cu_used = 0_u64;
        //
        //            for fee in &self.avg_fees {
        //                total_fee += fee.fee.total;
        //                total_cu_limit += fee.fee.cu_limit;
        //                total_cu_used += fee.fee.cu_used;
        //            }
        //            (total_fee / avg_fees_seconds, total_cu_limit / avg_fees_seconds, total_cu_used / avg_fees_seconds)
        //        }
        //        else {
        //            (0, 0, 0)
        //        };
        //
        //        println!("Avg Fee: {avg_fee}");
        //        println!("Avg CU Limit: {avg_cu_limit}");
        //        println!("Avg CU Used: {avg_cu_used}");
        //        println!("Avg Fee/CU Limit: {:0.9}", (avg_fee as f64) / (avg_cu_limit as f64));
        //        println!("Avg Fee/CU Used: {:0.9}", (avg_fee as f64) / (avg_cu_used as f64));

        // Remove tx that are old enough that they must have already landed if they're ever going to land,
        // and when removing them, add their fee details into groups.
        let retain_timestamp = now - TX_RETENTION_DURATION_MS;
        self.current_tx.retain(|_, tx| {
            if tx.submissions[0].timestamp < retain_timestamp {
                for i in 0..tx.submissions.len() {
                    let submission = &tx.submissions[i];
                    // Only the first submission gets the fee; everything else gets zero_fee (or if the tx never
                    // landed, of course the submission gets zero_fee)
                    let fee = if i == 0 { tx.fee.as_ref().unwrap_or(&self.zero_fee) } else { &self.zero_fee };
                    if let Some(fee_lamports_submitted_group) = &mut self.fee_lamports_submitted_group {
                        fee_lamports_submitted_group.add_value(submission.submitter, submission.timestamp, fee.total);
                    }
                    if let Some(fee_microlamports_per_cu_limit_landed_group) =
                        &mut self.fee_microlamports_per_cu_limit_landed_group
                    {
                        fee_microlamports_per_cu_limit_landed_group.add_value(
                            submission.submitter,
                            submission.timestamp,
                            (fee.total * 1000) / fee.cu_limit
                        );
                    }
                    if let Some(fee_microlamports_per_cu_used_landed_group) =
                        &mut self.fee_microlamports_per_cu_used_landed_group
                    {
                        fee_microlamports_per_cu_used_landed_group.add_value(
                            submission.submitter,
                            submission.timestamp,
                            (fee.total * 1000) / fee.cu_used
                        );
                    }
                }
                false
            }
            else {
                true
            }
        });

        // Do group periodic work
        if let Some(failed_exceeded_quic_connections_group) = &mut self.failed_exceeded_quic_connections_group {
            failed_exceeded_quic_connections_group.periodic(&self.stakes, now);
        }

        if let Some(useless_quic_connections_group) = &mut self.useless_quic_connections_group {
            useless_quic_connections_group.periodic(&self.stakes, now);
        }

        if let Some(fee_lamports_submitted_group) = &mut self.fee_lamports_submitted_group {
            fee_lamports_submitted_group.periodic(&self.stakes, now);
        }

        if let Some(fee_microlamports_per_cu_limit_landed_group) = &mut self.fee_microlamports_per_cu_limit_landed_group
        {
            fee_microlamports_per_cu_limit_landed_group.periodic(&self.stakes, now);
        }

        if let Some(fee_microlamports_per_cu_used_landed_group) = &mut self.fee_microlamports_per_cu_used_landed_group {
            fee_microlamports_per_cu_used_landed_group.periodic(&self.stakes, now);
        }

        // Remove peers whose most recent timestamp is older than 3 days old
        let retain_timestamp = now - PEER_RETENTION_DURATION_MS;
        self.peers.retain(|ip_addr, peer| {
            if peer.most_recent_timestamp < retain_timestamp {
                self.stakes.remove(ip_addr);
                false
            }
            else {
                true
            }
        });
    }
}
