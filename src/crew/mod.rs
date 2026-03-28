//! Multi-agent crews: declarative specs, orchestration, P2P hooks.

pub mod orchestrator;
pub mod p2p;
pub mod spec;
pub mod store;

pub use orchestrator::{run_crew, CrewOutput, CrewTaskOutput};
pub use p2p::{
    world_topic, CampaignMilestone, CrewTaskClaim, CrewTaskOffer, CrewTaskResult,
    PodArtifactPublished, CREW_TASK_TOPIC, POD_TOPIC,
};
pub use spec::{CrewAgentDef, CrewProcess, CrewSpec, CrewTaskDef};
pub use store::{CrewRunRecord, CrewRunStore};
