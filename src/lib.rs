//! The neighborhood from the SDK's `tests/mesh_forwarding.rs`, driven from a
//! page instead of a test.
//!
//! Every device is a real `OfflineProtocol` over a `MockTransport`, and a
//! round moves frames exactly as the test harness does: whatever a device
//! hands to a neighbor reaches that neighbor and nobody else, unless the air
//! loses or delays it. Nothing about forwarding is re-implemented here, so
//! what the page shows is what the engine does.
//!
//! The page owns geometry (where devices stand, who is in range); this owns
//! the engines and the air.

#![deny(unsafe_code)]

use offline_protocol::{OfflineProtocol, ProtocolConfig};
use offline_protocol_core::Message;
use offline_protocol_transport::{mock::MockTransport, Transport, TransportType};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::sync::{Arc, Mutex};
use wasm_bindgen::prelude::*;

/// A simulated neighborhood the page can reshape between rounds.
#[wasm_bindgen]
pub struct MeshSim {
    nodes: BTreeMap<String, OfflineProtocol>,
    radios: BTreeMap<String, MockTransport>,
    links: BTreeMap<String, BTreeSet<String>>,
    /// Frames on the air, as `(round due, from, to, frame)`.
    in_flight: Vec<(u64, String, String, Message)>,
    /// What every engine emitted since the last round, as `{node, event}`.
    events: Arc<Mutex<Vec<Value>>>,
    /// Ids the page sent, so a frame can be told apart from control traffic.
    sent: HashSet<String>,
    round: u64,
    loss: f64,
    delay: (u64, u64),
    seed: u64,
}

#[wasm_bindgen]
impl MeshSim {
    /// An empty neighborhood whose losses and delays are drawn from `seed`.
    #[wasm_bindgen(constructor)]
    pub fn new(seed: u32) -> MeshSim {
        MeshSim {
            nodes: BTreeMap::new(),
            radios: BTreeMap::new(),
            links: BTreeMap::new(),
            in_flight: Vec::new(),
            events: Arc::new(Mutex::new(Vec::new())),
            sent: HashSet::new(),
            round: 0,
            loss: 0.0,
            delay: (0, 0),
            seed: u64::from(seed),
        }
    }

    /// Adds a device with the configuration every test device starts from.
    #[wasm_bindgen(js_name = addNode)]
    pub fn add_node(&mut self, id: &str) -> Result<(), String> {
        if self.nodes.contains_key(id) {
            return Err(format!("{id} already exists"));
        }
        let mut config = ProtocolConfig::new("mesh-sim", id);
        // The harness is about who carries what, not about encryption.
        config.encryption.require_encryption = false;
        // No hold before forwarding: rounds are the clock the page shows.
        config.mesh_relay.jitter_min = core::time::Duration::ZERO;
        config.mesh_relay.jitter_max = core::time::Duration::ZERO;

        let radio = MockTransport::new(TransportType::BLE);
        radio.start().map_err(|e| e.to_string())?;
        // A device can only put a frame on a link it actually has.
        radio.set_reject_unknown_recipients(true);
        let handle = radio.clone();

        let mut node = OfflineProtocol::new(config).map_err(|e| e.to_string())?;
        node.transport_manager_mut()
            .add_transport(TransportType::BLE, Box::new(radio));
        node.start().map_err(|e| e.to_string())?;
        let sink = Arc::clone(&self.events);
        let me = id.to_string();
        node.on_event(move |event| {
            if let Ok(event) = serde_json::to_value(&event) {
                lock(&sink).push(json!({ "node": me, "event": event }));
            }
        });

        self.nodes.insert(id.to_string(), node);
        self.radios.insert(id.to_string(), handle);
        self.links.insert(id.to_string(), BTreeSet::new());
        Ok(())
    }

    /// Takes a device out of range of everyone and switches it off.
    #[wasm_bindgen(js_name = removeNode)]
    pub fn remove_node(&mut self, id: &str) {
        let peers: Vec<String> = self.links.get(id).into_iter().flatten().cloned().collect();
        for peer in peers {
            self.unlink(id, &peer);
        }
        self.nodes.remove(id);
        self.radios.remove(id);
        self.links.remove(id);
    }

    /// Puts two devices in range of each other.
    pub fn link(&mut self, a: &str, b: &str) {
        if a == b || !self.nodes.contains_key(a) || !self.nodes.contains_key(b) {
            return;
        }
        if !self.links.entry(a.into()).or_default().insert(b.into()) {
            return;
        }
        self.links.entry(b.into()).or_default().insert(a.into());
        self.radios[a].add_connected_peer(b, -55);
        self.radios[b].add_connected_peer(a, -55);
        for (me, peer) in [(a, b), (b, a)] {
            if let Some(node) = self.nodes.get_mut(me) {
                node.on_neighbor_discovered(peer);
            }
        }
    }

    /// Takes two devices out of range of each other.
    pub fn unlink(&mut self, a: &str, b: &str) {
        let removed = self.links.get_mut(a).is_some_and(|l| l.remove(b));
        if !removed {
            return;
        }
        if let Some(l) = self.links.get_mut(b) {
            l.remove(a);
        }
        for (me, peer) in [(a, b), (b, a)] {
            if let Some(radio) = self.radios.get(me) {
                radio.remove_connected_peer(peer);
            }
            if let Some(node) = self.nodes.get_mut(me) {
                node.on_neighbor_lost(peer);
            }
        }
    }

    /// What the air does to a frame: the share lost, and the extra rounds one
    /// spends in flight, drawn uniformly from `[delay_min, delay_max]`.
    #[wasm_bindgen(js_name = setAir)]
    pub fn set_air(&mut self, loss: f64, delay_min: u32, delay_max: u32) {
        self.loss = loss.clamp(0.0, 1.0);
        let low = u64::from(delay_min.min(delay_max));
        self.delay = (low, u64::from(delay_max.max(delay_min)));
    }

    /// Sends a message the way an app would, returning its id.
    pub fn send(&mut self, from: &str, to: &str, content: &str) -> Result<String, String> {
        let node = self
            .nodes
            .get_mut(from)
            .ok_or_else(|| format!("no device {from}"))?;
        let id = node
            .send_message(to, content, None, None::<String>)
            .map_err(|e| e.to_string())?
            .as_str();
        self.sent.insert(id.clone());
        Ok(id)
    }

    /// Runs one round and reports it as JSON:
    /// `{round, frames, deliveries, events, inFlight}`.
    ///
    /// `frames` is every hand-off put on the air this round, each with
    /// `{from, to, id, about, kind, hops, lost, rounds}`: `about` is the
    /// message a receipt answers (or the frame's own id), `kind` is
    /// `message`, `receipt` or `control`, and `rounds` is how long it flies.
    pub fn step(&mut self) -> String {
        self.round += 1;
        let ids: Vec<String> = self.nodes.keys().cloned().collect();

        let mut deliveries = Vec::new();
        for id in &ids {
            let Some(node) = self.nodes.get_mut(id) else {
                continue;
            };
            while let Some(m) = node.receive_message() {
                deliveries.push(json!({
                    "node": id,
                    "from": m.sender.as_str(),
                    "id": m.id.as_str(),
                    "content": m.content,
                    "hops": m.hop_count.value(),
                }));
            }
            if let Err(e) = node.process() {
                lock(&self.events).push(json!({ "node": id, "error": e.to_string() }));
            }
        }

        let mut frames = Vec::new();
        let mut draw = Draw(self.seed ^ self.round.wrapping_mul(0x2545_F491_4F6C_DD1D));
        for from in &ids {
            let radio = &self.radios[from];
            let mut handed: Vec<(String, Message)> = radio
                .sent_messages()
                .into_iter()
                .map(|m| (m.recipient.as_str().to_string(), m))
                .collect();
            handed.extend(radio.peer_sends());
            radio.clear_sent_messages();
            radio.clear_peer_sends();

            for (to, message) in handed {
                let id = message.id.as_str();
                let (kind, about) = match message.metadata.get("ack_for") {
                    Some(about) => ("receipt", about.clone()),
                    None if self.sent.contains(&id) => ("message", id.clone()),
                    None => ("control", id.clone()),
                };
                // A lost frame was still transmitted: it cost the airtime.
                let lost = draw.unit() < self.loss;
                let rounds = 1 + draw.between(self.delay);
                frames.push(json!({
                    "from": from, "to": to, "id": id, "about": about, "kind": kind,
                    "hops": message.hop_count.value(), "lost": lost, "rounds": rounds,
                }));
                if !lost {
                    let due = self.round + rounds - 1;
                    self.in_flight.push((due, from.clone(), to, message));
                }
            }
        }

        // Land what is due. Nothing arrives over a link that has since gone.
        let round = self.round;
        let (landing, flying): (Vec<_>, Vec<_>) = std::mem::take(&mut self.in_flight)
            .into_iter()
            .partition(|(due, ..)| *due <= round);
        self.in_flight = flying;
        for (_, from, to, message) in landing {
            if self.links.get(&from).is_some_and(|l| l.contains(&to)) {
                self.radios[&to].queue_message_from(message, from);
            }
        }

        let events = std::mem::take(&mut *lock(&self.events));
        json!({
            "round": self.round,
            "frames": frames,
            "deliveries": deliveries,
            "events": events,
            "inFlight": self.in_flight.len(),
        })
        .to_string()
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// SplitMix64, as in the SDK's harness, so a seed's losses repeat.
struct Draw(u64);

impl Draw {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }

    fn between(&mut self, (low, high): (u64, u64)) -> u64 {
        low + self.next() % (high - low + 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// alice — bob — carol: the message is carried two hops and its receipt
    /// comes back, which is the whole loop the page animates.
    #[test]
    fn a_message_is_carried_and_answered() {
        let mut sim = MeshSim::new(1);
        for id in ["alice", "bob", "carol"] {
            sim.add_node(id).expect("add");
        }
        sim.link("alice", "bob");
        sim.link("bob", "carol");
        let id = sim.send("alice", "carol", "hi").expect("send");

        let (mut received, mut delivered, mut carried_back) = (false, false, false);
        for _ in 0..20 {
            let round: Value = serde_json::from_str(&sim.step()).expect("json");
            received |= round["deliveries"]
                .as_array()
                .expect("deliveries")
                .iter()
                .any(|d| d["node"] == "carol" && d["content"] == "hi");
            delivered |= round["events"].as_array().expect("events").iter().any(|e| {
                e["node"] == "alice"
                    && e["event"]["type"] == "message_delivered"
                    && e["event"]["message_id"] == id.as_str()
            });
            carried_back |= round["frames"].as_array().expect("frames").iter().any(|f| {
                f["kind"] == "receipt" && f["from"] == "bob" && f["to"] == "alice"
            });
        }
        assert!(received, "carol must receive it");
        assert!(carried_back, "the receipt must be carried back by bob");
        assert!(delivered, "alice must learn it was delivered");
    }
}
