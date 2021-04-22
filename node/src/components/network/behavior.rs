use std::{
    collections::VecDeque,
    task::{Context, Poll},
};

use derive_more::From;
use libp2p::{
    core::PublicKey,
    gossipsub::{Gossipsub, GossipsubEvent},
    identify::{Identify, IdentifyEvent},
    kad::{record::store::MemoryStore, Kademlia, KademliaEvent},
    request_response::{RequestResponse, RequestResponseEvent},
    swarm::{NetworkBehaviourAction, NetworkBehaviourEventProcess, PollParameters},
    Multiaddr, NetworkBehaviour, PeerId,
};
use tracing::{debug, trace, warn};

use super::{
    gossip::{self, TOPIC},
    one_way_messaging, peer_discovery, Config, GossipMessage, OneWayCodec, OneWayOutgoingMessage,
};
use crate::{
    components::networking_metrics::NetworkingMetrics,
    types::{Chainspec, NodeId},
};

/// An enum defining the top-level events passed to the swarm's handler.  This will be received in
/// the swarm's handler wrapped in a `SwarmEvent::Behaviour`.
#[derive(Debug, From)]
pub(super) enum SwarmBehaviorEvent {
    OneWayMessaging(RequestResponseEvent<Vec<u8>, ()>),
    Gossiper(GossipsubEvent),
    Kademlia(KademliaEvent),
    Identify(IdentifyEvent),
}

/// The top-level behavior used in the libp2p swarm.  It holds all subordinate behaviors required to
/// operate the network component.
#[derive(NetworkBehaviour)]
#[behaviour(out_event = "SwarmBehaviorEvent", poll_method = "custom_poll")]
pub(super) struct Behavior {
    one_way_message_behavior: RequestResponse<OneWayCodec>,
    gossip_behavior: Gossipsub,
    kademlia_behavior: Kademlia<MemoryStore>,
    identify_behavior: Identify,
    #[behaviour(ignore)]
    our_id: NodeId,
    /// Events generated by the behavior that are pending a poll.
    #[behaviour(ignore)]
    events: VecDeque<SwarmBehaviorEvent>,
}

impl Behavior {
    pub(super) fn new(
        config: &Config,
        net_metrics: &NetworkingMetrics,
        chainspec: &Chainspec,
        our_public_key: PublicKey,
    ) -> Self {
        let one_way_message_behavior =
            one_way_messaging::new_behavior(config, net_metrics, chainspec);

        let gossip_behavior = gossip::new_behavior(config, chainspec, our_public_key.clone());

        let (kademlia_behavior, identify_behavior) =
            peer_discovery::new_behaviors(config, chainspec, our_public_key.clone());

        Behavior {
            one_way_message_behavior,
            gossip_behavior,
            kademlia_behavior,
            identify_behavior,
            our_id: NodeId::P2p(PeerId::from(our_public_key)),
            events: VecDeque::new(),
        }
    }

    /// Sends the given message out.
    pub(super) fn send_one_way_message(&mut self, outgoing_message: OneWayOutgoingMessage) {
        let request_id = self
            .one_way_message_behavior
            .send_request(&outgoing_message.destination, outgoing_message.message);
        trace!("{}: sent one-way message {}", self.our_id, request_id);
    }

    /// Adds the given peer's details to the kademlia routing table and bootstraps kademlia if this
    /// is the first peer added.
    ///
    /// While bootstrapping is not strictly required, it will normally greatly speed up the process
    /// of populating the routing table's k-buckets.
    ///
    /// We assume that calling bootstrap multiple times will not be problematic, although this will
    /// not normally happen.
    pub(super) fn add_discovered_peer(
        &mut self,
        peer_id: &PeerId,
        listening_addresses: Vec<Multiaddr>,
    ) {
        let should_bootstrap = self
            .kademlia_behavior
            .kbuckets()
            .map(|k_bucket| k_bucket.num_entries())
            .sum::<usize>()
            == 1;

        for address in listening_addresses {
            self.kademlia_behavior.add_address(peer_id, address);
        }

        if should_bootstrap {
            debug!("{}: bootstrapping kademlia", self.our_id);
            if self.kademlia_behavior.bootstrap().is_err() {
                warn!(
                    "{}: could not bootstrap kademlia due to lost connection leaving no peers",
                    self.our_id
                )
            }
        }
    }

    /// Performs a random kademlia lookup in order to refresh the routing table.
    pub(super) fn discover_peers(&mut self) {
        let random_address = PeerId::random();
        let query_id = self.kademlia_behavior.get_closest_peers(random_address);
        debug!(
            "{}: random kademlia lookup for peers closest to {} with {:?}",
            self.our_id, random_address, query_id
        );
    }

    /// Initiates gossiping the given message.
    pub(super) fn gossip(&mut self, message: GossipMessage) {
        if let Err(error) = self.gossip_behavior.publish(TOPIC.clone(), message) {
            warn!(?error, "{}: failed to gossip new message", self.our_id);
        }
    }

    /// Polls the behavior for new events.
    fn custom_poll<T>(
        &mut self,
        _context: &mut Context,
        _parameters: &mut impl PollParameters,
    ) -> Poll<NetworkBehaviourAction<T, SwarmBehaviorEvent>> {
        if let Some(event) = self.events.pop_back() {
            Poll::Ready(NetworkBehaviourAction::GenerateEvent(event))
        } else {
            Poll::Pending
        }
    }
}

impl NetworkBehaviourEventProcess<RequestResponseEvent<Vec<u8>, ()>> for Behavior {
    fn inject_event(&mut self, event: RequestResponseEvent<Vec<u8>, ()>) {
        self.events.push_front(SwarmBehaviorEvent::from(event));
    }
}

impl NetworkBehaviourEventProcess<GossipsubEvent> for Behavior {
    fn inject_event(&mut self, event: GossipsubEvent) {
        self.events.push_front(SwarmBehaviorEvent::from(event));
    }
}

impl NetworkBehaviourEventProcess<KademliaEvent> for Behavior {
    fn inject_event(&mut self, event: KademliaEvent) {
        self.events.push_front(SwarmBehaviorEvent::from(event));
    }
}

impl NetworkBehaviourEventProcess<IdentifyEvent> for Behavior {
    fn inject_event(&mut self, event: IdentifyEvent) {
        self.events.push_front(SwarmBehaviorEvent::from(event));
    }
}
