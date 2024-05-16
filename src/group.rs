use std::collections::HashMap;
use std::net::IpAddr;

pub const DEFAULT_GROUP_EXPIRATION_SECONDS : u64 = 24 * 60 * 60; // One day

// A Group manages the membership of ip addresses in a classifier group.
#[derive(Default)]
pub struct Group
{
    name : String,

    // Map from member to timestamp of when the member will expire (in milliseconds)
    members : HashMap<IpAddr, u64>
}

impl Group
{
    pub fn new(name : &str) -> Self
    {
        Self { name : name.to_string(), members : Default::default() }
    }

    pub fn add(
        &mut self,
        ip_addr : IpAddr,
        expiration : u64
    )
    {
        self.members
            .entry(ip_addr)
            .and_modify(|timestamp| {
                if *timestamp < expiration {
                    println!("Update {ip_addr} in group {} with expiration {expiration}", self.name);
                    *timestamp = expiration
                }
            })
            .or_insert_with(|| {
                println!("Add {ip_addr} to group {} with expiration {expiration}", self.name);
                expiration
            });
    }

    // To be called once per second
    pub fn periodic(
        &mut self,
        now : u64
    )
    {
        // Expire group memberships that are too old
        self.members.retain(|ip_addr, expire_timestamp| {
            if *expire_timestamp < now {
                println!("Remove {ip_addr} from group {}", self.name);
                false
            }
            else {
                true
            }
        });
    }
}
