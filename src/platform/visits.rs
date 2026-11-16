//! Visit grants authorize play; they never change permanent membership or ownership.
use super::*;
use std::collections::BTreeSet;

#[derive(Clone, Serialize, Deserialize)]
pub struct Participant {
    pub after_message: u64,
    pub ended: bool,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Visit {
    pub name: String,
    pub host: String,
    pub participants: BTreeMap<String, Participant>,
    pub games: BTreeSet<String>,
    pub expires: u64,
    pub ended: bool,
}
impl Visit {
    pub fn active(&self, time: u64) -> bool {
        !self.ended && self.expires > time
    }
    pub fn participant(&self, email: &str) -> bool {
        self.participants.get(email).is_some_and(|p| !p.ended)
    }
}
impl Data {
    pub fn visit_access(&self, id: &str, email: &str, time: u64) -> Option<&Visit> {
        self.visits.get(id).filter(|v| {
            v.active(time)
                && self.profile_enabled(email)
                && (self.groups.member(email) == v.host || v.participant(email))
        })
    }
    pub fn active_visit(&self, host: &str, time: u64) -> Option<(&String, &Visit)> {
        self.visits
            .iter()
            .find(|(_, v)| v.host == host && v.active(time))
    }
    pub fn launch_allowed(&self, launch: &Launch, time: u64) -> bool {
        if !self.profile_enabled(&launch.email) {
            return false;
        }
        if self.source_app(launch.runtime_id()) != launch.app {
            return false;
        }
        if launch.visit.is_some() && !self.apps.get(&launch.app).is_some_and(|a| a.listed) {
            return false;
        }
        match &launch.visit {
            Some(id) => self.visit_access(id, &launch.email, time).is_some_and(|v| {
                self.deployment_group(launch.runtime_id()) == v.host
                    && (self.groups.app(&launch.app) == v.host || v.games.contains(&launch.app))
            }),
            None => launch.deployment.is_none() && self.groups.allows(&launch.email, &launch.app),
        }
    }
    pub fn group_participant(&self, email: &str, group: &str, time: u64) -> bool {
        self.profile_enabled(email)
            && (self.groups.member(email) == group
                || self
                    .active_visit(group, time)
                    .is_some_and(|(_, v)| v.participant(email)))
    }
}
