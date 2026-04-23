#![cfg(test)]
#![allow(deprecated)]

use super::*;
use soroban_sdk::testutils::{Address as _, Ledger};
use soroban_sdk::{token, Address, BytesN, Env, Vec};

// --- Helpers ---
fn device_key(env: &Env, byte: u8) -> BytesN<32> {
    BytesN::from_array(env, &[byte; 32])
}

fn create_token(env: &Env) -> Address {
    let admin = Address::generate(env);
    env.register_stellar_asset_contract_v2(admin).address()
}

// ==================== MOCK CONTRACTS ====================

mod mock_sorosusu {
    use soroban_sdk::{contract, contractimpl, Address, Env};

    #[contract]
    pub struct MockSoroSusu;

    #[contractimpl]
    impl MockSoroSusu {
        pub fn set_default(env: Env, user: Address, in_default: bool) {
            env.storage().instance().set(&user, &in_default);
        }

        pub fn is_in_default(env: Env, user: Address) -> bool {
            env.storage().instance().get(&user).unwrap_or(false)
        }

        pub fn is_trusted_saver(_env: Env, _user: Address) -> bool { false }
        pub fn get_susu_score(_env: Env, _user: Address) -> u32 { 0 }

        pub fn record_debt_payment(env: Env, user: Address, amount: i128) {
            let key = (user.clone(), soroban_sdk::symbol_short!("paid"));
            let current: i128 = env.storage().instance().get(&key).unwrap_or(0);
            env.storage().instance().set(&key, &current.saturating_add(amount));
        }
    }
}

#[test]
fn test_grace_period_expiration() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(UtilityContract, ());
    let client = UtilityContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let provider = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_address = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let token_admin_client = token::StellarAssetClient::new(&env, &token_address);

    token_admin_client.mint(&user, &2000);

    let device_public_key = device_key(&env, 1);
    // Integrated Seasonal/Sustainability params: end_date (0) and rent_deposit (0)
    let meter_id = client.register_meter(&user, &provider, &10, &token_address, &device_public_key, &0, &0);

    // Top up with balance to activate
    client.top_up(&meter_id, &500);
    let meter = client.get_meter(&meter_id).unwrap();
    assert!(meter.is_active);
    assert_eq!(meter.balance, 500);

    // Pair the meter
    client.initiate_pairing(&meter_id);
    client.complete_pairing(&meter_id, &BytesN::from_array(&env, &[2u8; 64]));

    // Use up balance exactly to 0 - should start grace period
    env.ledger().set_timestamp(env.ledger().timestamp() + 50); 
    client.claim(&meter_id);

    let meter = client.get_meter(&meter_id).unwrap();
    assert_eq!(meter.balance, 0);
    assert!(meter.is_active); 
    assert!(meter.grace_period_start > 0); 

    // Fast forward another 25 hours - should expire grace period
    env.ledger().set_timestamp(env.ledger().timestamp() + (25 * 60 * 60));
    client.claim(&meter_id); 

    let meter = client.get_meter(&meter_id).unwrap();
    assert!(!meter.is_active); 
}

#[test]
fn test_peak_hour_tariff() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(UtilityContract, ());
    let client = UtilityContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let provider = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_address = env.register_stellar_asset_contract_v2(token_admin.clone()).address();
    let token = token::Client::new(&env, &token_address);
    let token_admin_client = token::StellarAssetClient::new(&env, &token_address);

    token_admin_client.mint(&user, &5000);

    let rate = 10; 
    let device_public_key = device_key(&env, 1);
    let meter_id = client.register_meter(&user, &provider, &rate, &token_address, &device_public_key, &0, &0);

    client.initiate_pairing(&meter_id);
    client.complete_pairing(&meter_id, &BytesN::from_array(&env, &[2u8; 64]));

    client.initiate_pairing(&meter_id);
    client.complete_pairing(&meter_id, &BytesN::from_array(&env, &[2u8; 64]));
    client.top_up(&meter_id, &5000);

    // 19:00 UTC Peak hours
    env.ledger().set_timestamp(68400);

    let signed_data = SignedUsageData {
        meter_id,
        timestamp: 68400,
        watt_hours_consumed: 1000,
        units_consumed: 10,
        is_renewable_energy: false,
        signature: BytesN::from_array(&env, &[3u8; 64]),
        public_key: device_public_key,
    };
    client.deduct_units(&signed_data);

    let meter = client.get_meter(&meter_id).unwrap();
    // Base cost 100 * 1.5 multiplier = 150
    assert_eq!(meter.balance, 4850); 
}

#[test]
fn test_green_energy_bonus() {
    let env = Env::default();
    let contract_id = env.register(UtilityContract, ());
    let client = UtilityContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let provider = Address::generate(&env);
    let token_address = create_token(&env);

    let susu_id = env.register_contract(None, mock_sorosusu::MockSoroSusu);
    let susu_client = mock_sorosusu::MockSoroSusuClient::new(&env, &susu_id);

    let user = Address::generate(&env);
    let provider = Address::generate(&env);
    let token_address = create_token(&env);
    let token_admin = token::StellarAssetClient::new(&env, &token_address);

    token_admin.mint(&user, &100_000);
    client.set_tax_rate(&0);
    client.set_sorosusu_contract(&susu_id);

    let meter_id = client.register_meter(&user, &provider, &10, &token_address, &device_key(&env, 42));
    client.top_up(&meter_id, &100_000);

    // Generate maintenance fund via claim
    env.ledger().set_timestamp(1_000);
    client.claim(&meter_id);

    let fund_before = client.get_maintenance_fund(&meter_id);
    susu_client.set_default(&user, &true);

    client.service_sorosusu_debt(&meter_id);

    let fund_after = client.get_maintenance_fund(&meter_id);
    assert!(fund_after < fund_before);
}

// ==================== PROVIDER RELIABILITY TESTS ====================

#[test]
fn test_reliability_score_logic() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, UtilityContract);
    let client = UtilityContractClient::new(&env, &contract_id);
    let provider = Address::generate(&env);

    // Report 99 online windows out of 100
    for _ in 0..99u32 {
        client.report_provider_uptime(&provider, &true);
    }
    client.report_provider_uptime(&provider, &false);

    let score = client.get_reliability_score(&provider).unwrap();
    assert_eq!(score.score_bps, 9900);
    assert_eq!(score.badge, ReliabilityBadge::Gold);
}

#[test]
fn test_reliability_score_reset_impact() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, UtilityContract);
    let client = UtilityContractClient::new(&env, &contract_id);
    let provider = Address::generate(&env);

    // Create meter info vector
    let mut meter_infos = Vec::new(&env);
    meter_infos.push_back(MeterInfo {
        user: user1.clone(),
        provider: provider.clone(),
        off_peak_rate: 100,
        token: token_address.clone(),
        billing_type: BillingType::PrePaid,
        device_public_key: device_key1,
    });
    meter_infos.push_back(MeterInfo {
        user: user2.clone(),
        provider: provider.clone(),
        off_peak_rate: 200,
        token: token_address.clone(),
        billing_type: BillingType::PostPaid,
        device_public_key: device_key2,
    });
    meter_infos.push_back(MeterInfo {
        user: user3.clone(),
        provider: provider.clone(),
        off_peak_rate: 150,
        token: token_address.clone(),
        billing_type: BillingType::PrePaid,
        device_public_key: device_key3,
    });

    // Call batch_register_meters
    let batch_event = client.batch_register_meters(&meter_infos);

    // Verify batch event
    assert_eq!(batch_event.start_id, 1);
    assert_eq!(batch_event.end_id, 3);
    assert_eq!(batch_event.count, 3);

    // Verify individual meters were created
    let meter1 = client.get_meter(&1);
    assert!(meter1.is_some());
    let meter1 = meter1.unwrap();
    assert_eq!(meter1.user, user1);
    assert_eq!(meter1.off_peak_rate, 100);
    assert_eq!(meter1.billing_type, BillingType::PrePaid);

    let meter2 = client.get_meter(&2);
    assert!(meter2.is_some());
    let meter2 = meter2.unwrap();
    assert_eq!(meter2.user, user2);
    assert_eq!(meter2.off_peak_rate, 200);
    assert_eq!(meter2.billing_type, BillingType::PostPaid);

    let meter3 = client.get_meter(&3);
    assert!(meter3.is_some());
    let meter3 = meter3.unwrap();
    assert_eq!(meter3.user, user3);
    assert_eq!(meter3.off_peak_rate, 150);
    assert_eq!(meter3.billing_type, BillingType::PrePaid);
}

#[test]
fn test_batch_register_meters_empty_vector() {
    let env = Env::default();
    let contract_address = env.register_contract(None, UtilityContract);
    let client = UtilityContractClient::new(&env, &contract_address);
    
    // Test with empty vector should panic
    let result = env.try_invoke_contract::<_, _>(
        &contract_address,
        &soroban_sdk::Symbol::new(&env, "batch_register_meters"),
        (Vec::<MeterInfo>::new(&env),),
    );
    assert!(result.is_err());
}

#[test]
fn test_green_energy_bonus() {
    let env = Env::default();
    let contract_address = env.register_contract(None, UtilityContract);
    let client = UtilityContractClient::new(&env, &contract_address);

    let user = Address::generate(&env);
    let provider = Address::generate(&env);
    let token_address = Address::generate(&env);

    // Register a meter
    let meter_id = client.register_meter_with_mode(
        &user,
        &provider,
        &1000,
        &token_address,
        &BillingType::PrePaid,
        &device_key(&env, 0),
        &0, // end_date
        &0, // rent_deposit
    );

    client.set_green_energy_discount(&meter_id, &1000); // 10% discount
    client.top_up(&meter_id, &10000);

    let renewable_usage = SignedUsageData {
        meter_id: meter_id.clone(),
        timestamp: env.ledger().timestamp(),
        watt_hours_consumed: 100,
        units_consumed: 50,
        is_renewable_energy: true,
        signature: BytesN::from_array(&env, &[0; 64]),
        public_key: device_key(&env, 0),
    };

    client.deduct_units(&renewable_usage);
    let meter = client.get_meter(&meter_id).unwrap();
    // 50 units * 1000 rate = 50,000. 10% discount = 45,000 cost.
    // Note: Adjust math based on your specific implementation of balance/rates
    assert!(meter.usage_data.renewable_watt_hours > 0);
}

#[test]
fn test_multisig_withdrawal_full_flow() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(UtilityContract, ());
    let client = UtilityContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let provider = Address::generate(&env);
    let treasury = Address::generate(&env);
    let token_address = create_token(&env);

    let mut finance_wallets = Vec::new(&env);
    for _ in 0..5 { finance_wallets.push_back(Address::generate(&env)); }

    let device_public_key = device_key(&env, 1);
    let meter_id = client.register_meter(&user, &provider, &100, &token_address, &device_public_key, &0, &0);

    client.configure_multisig_withdrawal(&provider, &finance_wallets, &3, &100_000_00);

    let withdrawal_amount: i128 = 150_000_00;
    let request_id = client.propose_multisig_withdrawal(&provider, &meter_id, &withdrawal_amount, &treasury);

    // Approvals
    client.approve_multisig_withdrawal(&provider, &request_id);
    client.approve_multisig_withdrawal(&provider, &request_id);

    client.execute_multisig_withdrawal(&provider, &request_id);
    let request = client.get_withdrawal_request(&provider, &request_id);
    assert!(request.is_executed);
}

#[test]
fn test_seasonal_factor_affects_rate() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(UtilityContract, ());
    let client = UtilityContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let provider = Address::generate(&env);
    let token_address = create_token(&env);
    let token_admin = token::StellarAssetClient::new(&env, &token_address);

    token_admin.mint(&user, &10000);

    let meter_id = client.register_meter(&user, &provider, &10, &token_address, &device_key(&env, 1), &0, &0);
    client.top_up(&meter_id, &5000);

    // Seasonal factor: 150% (Summer demand)
    client.set_seasonal_factor(&150);
    env.ledger().set_timestamp(env.ledger().timestamp() + 10);
    client.claim(&meter_id);
    
    let token = token::Client::new(&env, &token_address);
    // 10s * (10 rate * 1.5 seasonal) = 150
    assert_eq!(token.balance(&provider), 150);
}