use std::collections::BTreeMap;

use aether_contracts::{ExecutionPlan, ExecutionTimeouts, RequestBody};
use serde_json::{json, Map, Value};
use uuid::Uuid;

use super::{
    GatewayControlAuthContext, GeminiVideoTaskSeed, LocalVideoTaskContentAction,
    LocalVideoTaskPersistence, LocalVideoTaskSeed, LocalVideoTaskSnapshot, LocalVideoTaskStatus,
    LocalVideoTaskTransport, OpenAiVideoTaskSeed, VideoTaskService,
};

mod fixtures;
mod plans;
mod projection;
mod sync;
