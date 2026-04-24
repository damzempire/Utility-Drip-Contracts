#![cfg(test)]

use crate::*;
use soroban_sdk::testutils::{Address as _, Ledger};
use soroban_sdk::{Address, BytesN, Env};

fn node_key(env: &Env, byte: u8) -> BytesN<32> {
    BytesN::from_array(env, &[byte; 32])
}

fn empty_sig(env: &Env) -> BytesN<64> {
    BytesN::from_array(env, &[0; 64])
}

fn submit_report(
    env: &Env,
    client: &UtilityContractClient,
    stream_id: u64,
    start_time: u64,
    end_time: u64,
    node_id: u8,
) {
    client.submit_stream_sla_report(&SignedStreamSLAReport {
        report: StreamDowntimeReport {
            stream_id,
            start_time,
            end_time,
        },
        signature: empty_sig(env),
        node_public_key: node_key(env, node_id),
    });
}

fn setup_stream(env: &Env) -> (UtilityContractClient<'_>, Address, Address) {
    env.mock_all_auths();
    env.ledger().set_timestamp(1_000);

    let contract_id = env.register_contract(None, UtilityContract);
    let client = UtilityContractClient::new(env, &contract_id);
    let admin = Address::generate(env);
    let provider = Address::generate(env);
    let payer = Address::generate(env);

    client.add_sla_node(&admin, &node_key(env, 1));
    client.add_sla_node(&admin, &node_key(env, 2));
    client.add_sla_node(&admin, &node_key(env, 3));

    client.create_continuous_stream(
        &7,
        &provider,
        &payer,
        &1_000,
        &StreamSLAConfig {
            threshold_seconds: 100,
            penalty_multiplier_bps: 5_000,
        },
    );

    (client, provider, payer)
}

#[test]
fn stream_sla_proven_downtime_reduces_billing_rate() {
    let env = Env::default();
    let (client, _, _) = setup_stream(&env);

    env.ledger().set_timestamp(1_010);
    assert_eq!(client.get_stream_total_charged(&7), 10_000);

    submit_report(&env, &client, 7, 0, 60, 1);
    submit_report(&env, &client, 7, 10, 70, 2);

    let flow_after_conflict = client.get_continuous_flow(&7).unwrap();
    assert_eq!(flow_after_conflict.current_tokens_per_second, 1_000);
    assert!(!flow_after_conflict.sla_state.is_penalty_active);

    submit_report(&env, &client, 7, 0, 60, 2);
    let flow_after_first_consensus = client.get_continuous_flow(&7).unwrap();
    assert_eq!(flow_after_first_consensus.sla_state.accumulated_downtime, 60);
    assert_eq!(flow_after_first_consensus.current_tokens_per_second, 1_000);

    submit_report(&env, &client, 7, 100, 160, 1);
    submit_report(&env, &client, 7, 100, 160, 3);

    let penalized_flow = client.get_continuous_flow(&7).unwrap();
    assert_eq!(penalized_flow.sla_state.accumulated_downtime, 120);
    assert!(penalized_flow.sla_state.is_penalty_active);
    assert_eq!(penalized_flow.current_tokens_per_second, 500);
    assert_eq!(penalized_flow.status, ContinuousStreamStatus::Penalized);

    env.ledger().set_timestamp(1_020);
    assert_eq!(client.get_stream_total_charged(&7), 15_000);
}

#[test]
fn stream_sla_penalty_math_avoids_underflow_panics() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(500);

    let contract_id = env.register_contract(None, UtilityContract);
    let client = UtilityContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let provider = Address::generate(&env);
    let payer = Address::generate(&env);

    client.add_sla_node(&admin, &node_key(&env, 1));
    client.add_sla_node(&admin, &node_key(&env, 2));

    client.create_continuous_stream(
        &11,
        &provider,
        &payer,
        &3,
        &StreamSLAConfig {
            threshold_seconds: 10,
            penalty_multiplier_bps: 0,
        },
    );

    submit_report(&env, &client, 11, 0, 10, 1);
    submit_report(&env, &client, 11, 0, 10, 2);

    let flow = client.get_continuous_flow(&11).unwrap();
    assert_eq!(flow.current_tokens_per_second, 0);
    assert!(flow.sla_state.is_penalty_active);

    env.ledger().set_timestamp(520);
    assert_eq!(client.get_stream_total_charged(&11), 0);
}

#[test]
fn stream_sla_stable_service_restores_baseline_rate() {
    let env = Env::default();
    let (client, _, _) = setup_stream(&env);

    submit_report(&env, &client, 7, 0, 60, 1);
    submit_report(&env, &client, 7, 0, 60, 2);
    submit_report(&env, &client, 7, 100, 160, 1);
    submit_report(&env, &client, 7, 100, 160, 3);

    let penalized_flow = client.get_continuous_flow(&7).unwrap();
    assert_eq!(penalized_flow.current_tokens_per_second, 500);

    env.ledger().set_timestamp(1_221);
    let restored_flow = client.get_continuous_flow(&7).unwrap();
    assert_eq!(restored_flow.current_tokens_per_second, 1_000);
    assert_eq!(restored_flow.status, ContinuousStreamStatus::Active);
    assert!(!restored_flow.sla_state.is_penalty_active);
    assert_eq!(restored_flow.sla_state.accumulated_downtime, 0);
}
