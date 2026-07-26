#![cfg(feature = "product")]

use psychevo::__product::{
    capabilities::{AgentDiscoveryOptions, SkillDiscoveryOptions},
    configuration::RuntimeProfileConfig,
    persistence::StateRuntime,
    runtime::{RunOptions, RuntimeTool},
};

#[test]
fn first_party_bridge_exposes_capabilities_through_one_product_facade() {
    fn owns<T>() {}

    owns::<AgentDiscoveryOptions>();
    owns::<SkillDiscoveryOptions>();
    owns::<RuntimeProfileConfig>();
    owns::<StateRuntime>();
    owns::<RunOptions>();
    owns::<RuntimeTool>();
}
