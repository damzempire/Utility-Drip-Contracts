#![no_std]
use soroban_sdk::xdr::ToXdr;
use soroban_sdk::{
    contract, contractclient, contracterror, contractimpl, contracttype, panic_with_error, symbol_short, token,
    Address, Env, BytesN, Vec, Symbol, Bytes,
};

// --- Constants ---
const DEFAULT_BUFFER_DAYS: i128 = 3;
const TRUSTED_BUFFER_DAYS: i128 = 1;
const MINIMUM_BALANCE_TO_FLOW: i128 = 500;
const HOUR_IN_SECONDS: u64 = 60 * 60;
const DAY_IN_SECONDS: u64 = 24 * HOUR_IN_SECONDS;
const GRACE_PERIOD_SECONDS: u64 = 86_400;
const HEARTBEAT_THRESHOLD_SECONDS: u64 = 300;
const DEBT_THRESHOLD: i128 = -10_000_000;
const MAX_USAGE_PER_UPDATE: i128 = 1_000_000_000_000i128;
const MAX_TIMESTAMP_DELAY: u64 = 300;
const PEAK_HOUR_START: u64 = 18 * HOUR_IN_SECONDS;
const PEAK_HOUR_END: u64 = 21 * HOUR_IN_SECONDS;
const PEAK_RATE_MULTIPLIER: i128 = 3; 
const RATE_PRECISION: i128 = 2;
const XLM_PRECISION: i128 = 10_000_000;
const DEFAULT_TAX_RATE_BPS: i128 = 500;
const MAINTENANCE_FUND_PERCENT_BPS: i128 = 1;
const LEDGER_LIFETIME_EXTENSION: u32 = 1_000_000;
const AUTO_EXTEND_LEDGER_THRESHOLD: u32 = 500_000;
const UPGRADE_VETO_PERIOD_SECONDS: u64 = 7 * DAY_IN_SECONDS;
const ADMIN_TRANSFER_TIMELOCK: u64 = 48 * HOUR_IN_SECONDS;
const VETO_THRESHOLD_BPS: i128 = 1000;
const WITHDRAWAL_REQUEST_EXPIRY: u64 = 7 * DAY_IN_SECONDS;
const MIN_FINANCE_WALLETS: usize = 3;
const MAX_FINANCE_WALLETS: usize = 5;
const REFERRAL_REWARD_UNITS: i128 = 500;
const MAX_RESELLER_FEE_BPS: i128 = 2000;
const THROTTLING_THRESHOLD_PERCENT: i128 = 20;
const LOW_PRIORITY_THRESHOLD: u32 = 100;

// --- Modules ---
mod gas_estimator;
pub mod grant_stream_listener;
pub mod velocity_limit;

use gas_estimator::GasCostEstimator;
use velocity_limit::{check_velocity_limits, apply_override, revoke_override, get_velocity_config, set_velocity_config};

// --- Data Structures ---

#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum BillingType { PrePaid, PostPaid }

#[contracttype]
#[derive(Clone)]
pub struct UsageData {
    pub total_watt_hours: i128,
    pub current_cycle_watt_hours: i128,
    pub peak_usage_watt_hours: i128,
    pub last_reading_timestamp: u64,
    pub precision_factor: i128,
    pub renewable_watt_hours: i128,
    pub renewable_percentage: i128,
    pub monthly_volume: i128,
    pub last_volume_reset: u64,
    pub first_reading_timestamp: u64,
}

#[contracttype]
#[derive(Clone)]
pub struct UsageReport {
    pub meter_id: u64,
    pub timestamp: u64,
    pub watt_hours_consumed: i128,
    pub units_consumed: i128,
    pub is_renewable_energy: bool,
}

#[contracttype]
#[derive(Clone)]
pub struct SignedUsageData {
    pub meter_id: u64,
    pub timestamp: u64,
    pub watt_hours_consumed: i128,
    pub units_consumed: i128,
    pub signature: BytesN<64>,
    pub public_key: BytesN<32>,
    pub is_renewable_energy: bool,
}

#[contracttype]
#[derive(Clone)]
pub struct Meter {
    pub user: Address,
    pub provider: Address,
    pub billing_type: BillingType,
    pub off_peak_rate: i128,
    pub peak_rate: i128,
    pub rate_per_unit: i128,
    pub balance: i128,
    pub debt: i128,
    pub last_update: u64,
    pub is_active: bool,
    pub token: Address,
    pub usage_data: UsageData,
    pub device_public_key: BytesN<32>,
    pub end_date: u64,
    pub rent_deposit: i128,
    pub priority_index: u32,
    pub green_energy_discount_bps: i128,
    pub is_paused: bool,
    pub is_disputed: bool,
    pub challenge_timestamp: u64,
    pub credit_drip_rate: i128,
    pub is_closed: bool,
    pub off_peak_reward_rate_bps: i128,
    pub milestone_deadline: u64,
    pub milestone_confirmed: bool,
    pub rate_per_second: i128,
    pub collateral_limit: i128,
    pub max_flow_rate_per_hour: i128,
    pub last_claim_time: u64,
    pub claimed_this_hour: i128,
    pub is_paired: bool,
    pub tier_threshold: i128,
    pub tier_rate: i128,
    pub last_heartbeat: u64,
    pub grace_period_start: u64,
    pub is_offline: bool,
    pub estimated_usage_total: i128,
    pub sla_config: SLAConfig,
    pub sla_state: SLAState,
    pub parent_account: Option<Address>,
    pub heartbeat: u64,
}

#[contracttype]
#[derive(Clone)]
pub struct SLAConfig {
    pub threshold_seconds: u64,
    pub penalty_multiplier_bps: i128,
}

#[contracttype]
#[derive(Clone)]
pub struct SLAState {
    pub accumulated_downtime: u64,
    pub last_report_timestamp: u64,
    pub is_penalty_active: bool,
}

#[contracttype]
#[derive(Clone)]
pub struct SLADowntimeReport {
    pub meter_id: u64,
    pub start_time: u64,
    pub end_time: u64,
}

#[contracttype]
#[derive(Clone)]
pub struct SignedSLAReport {
    pub report: SLADowntimeReport,
    pub signature: BytesN<64>,
    pub node_public_key: BytesN<32>,
}

#[contracttype]
#[derive(Clone)]
pub struct ClaimSettlement {
    pub gross_claimed: i128,
    pub provider_payout: i128,
    pub tax_amount: i128,
    pub protocol_fee: i128,
    pub reseller_payout: i128,
}

#[contracttype]
#[derive(Clone)]
pub struct ResellerConfig {
    pub reseller: Address,
    pub fee_bps: i128,
}

#[contracttype]
#[derive(Clone)]
pub struct ConservationGoal {
    pub goal_id: u64,
    pub provider: Address,
    pub target_water_savings: i128,
    pub current_savings: i128,
    pub deadline: u64,
    pub is_active: bool,
    pub grant_amount: i128,
    pub grant_token: Address,
    pub created_at: u64,
    pub achieved_at: Option<u64>,
}

#[contracttype]
#[derive(Clone)]
pub struct GoalReachedEvent {
    pub goal_id: u64,
    pub provider: Address,
    pub water_savings: i128,
    pub grant_amount: i128,
    pub grant_token: Address,
    pub achieved_at: u64,
}

#[contracttype]
#[derive(Clone)]
pub struct OfflineReconciliation {
    pub meter_id: u64,
    pub estimated_cost: i128,
    pub actual_cost: i128,
    pub adjustment: i128,
    pub timestamp: u64,
}

#[contracttype]
#[derive(Clone)]
pub struct ZKUsageReport {
    pub commitment: BytesN<32>,
    pub nullifier: BytesN<32>,
    pub encrypted_usage: Bytes,
    pub proof_hash: BytesN<32>,
    pub meter_id: u64,
    pub billing_cycle: u32,
    pub timestamp: u64,
    pub is_verified: bool,
}

#[contracttype]
#[derive(Clone)]
pub struct ZKProof {
    pub commitment: BytesN<32>,
    pub nullifier: BytesN<32>,
    pub proof: Bytes,
    pub meter_id: u64,
    pub timestamp: u64,
    pub is_valid: bool,
}

#[contracttype]
#[derive(Clone)]
pub struct PrivateBillingStatus {
    pub meter_id: u64,
    pub billing_cycle: u32,
    pub total_commitments: u32,
    pub verified_proofs: u32,
    pub last_verification: u64,
    pub privacy_enabled: bool,
}

#[contracttype]
#[derive(Clone)]
pub struct MeterStatus {
    pub meter_id: u64,
    pub is_active: bool,
    pub balance: i128,
    pub billing_cycle: u32,
    pub total_commitments: u32,
    pub verified_proofs: u32,
    pub privacy_enabled: bool,
    pub last_update: u64,
    pub usage_summary: UsageData,
}

#[contracttype]
#[derive(Clone)]
pub struct MultiSigConfig {
    pub provider: Address,
    pub finance_wallets: Vec<Address>,
    pub required_signatures: u32,
    pub threshold_amount: i128,
    pub is_active: bool,
    pub created_at: u64,
}

#[contracttype]
#[derive(Clone)]
pub struct WithdrawalRequest {
    pub request_id: u64,
    pub provider: Address,
    pub meter_id: u64,
    pub amount_usd_cents: i128,
    pub destination: Address,
    pub proposer: Address,
    pub created_at: u64,
    pub expires_at: u64,
    pub approval_count: u32,
    pub is_executed: bool,
    pub is_cancelled: bool,
}

#[contracttype]
#[derive(Clone)]
pub struct UpgradeProposal {
    pub new_wasm_hash: BytesN<32>,
    pub proposed_at: u64,
    pub veto_deadline: u64,
    pub proposer: Address,
}

#[contracttype]
#[derive(Clone)]
pub struct AdminTransferProposal {
    pub current_admin: Address,
    pub proposed_admin: Address,
    pub proposed_at: u64,
    pub execution_deadline: u64,
    pub veto_count: u32,
    pub is_active: bool,
}

#[contracttype]
#[derive(Clone)]
pub struct LegalFreeze {
    pub meter_id: u64,
    pub frozen_at: u64,
    pub reason: soroban_sdk::String,
    pub compliance_officer: Address,
    pub legal_vault: Address,
    pub frozen_amount: i128,
    pub is_released: bool,
}

#[contracttype]
#[derive(Clone)]
pub struct VerifiedProvider {
    pub address: Address,
    pub is_verified: bool,
    pub verified_at: u64,
    pub verification_method: VerificationMethod,
    pub provider_name: soroban_sdk::String,
}

#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum VerificationMethod { IdentityVerified, CommunityVoted }

#[contracttype]
#[derive(Clone)]
pub struct SubDaoConfig {
    pub parent_dao: Address,
    pub sub_dao: Address,
    pub allocated_budget: i128,
    pub spent_budget: i128,
    pub token: Address,
    pub created_at: u64,
    pub is_active: bool,
}

#[contracttype]
#[derive(Clone)]
pub struct BillingGroup {
    pub parent_account: Address,
    pub child_meters: Vec<u64>,
    pub created_at: u64,
}

#[contracttype]
#[derive(Clone)]
pub struct WebhookConfig {
    pub url: soroban_sdk::String,
    pub user: Address,
    pub is_active: bool,
    pub created_at: u64,
}

#[contracttype]
#[derive(Clone)]
pub struct LowBalanceAlert {
    pub meter_id: u64,
    pub user: Address,
    pub remaining_balance: i128,
    pub hours_remaining: u32,
    pub timestamp: u64,
}

#[contracttype]
#[derive(Clone)]
pub struct ProviderWithdrawalWindow {
    pub daily_withdrawn: i128,
    pub last_reset: u64,
}

#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContinuousStreamStatus {
    Active,
    Penalized,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StreamSLAConfig {
    pub threshold_seconds: u64,
    pub penalty_multiplier_bps: i128,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StreamSLAState {
    pub accumulated_downtime: u64,
    pub last_report_timestamp: u64,
    pub is_penalty_active: bool,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContinuousFlow {
    pub stream_id: u64,
    pub provider: Address,
    pub payer: Address,
    pub baseline_tokens_per_second: i128,
    pub current_tokens_per_second: i128,
    pub total_charged: i128,
    pub last_rate_sync_timestamp: u64,
    pub created_at: u64,
    pub status: ContinuousStreamStatus,
    pub sla_config: StreamSLAConfig,
    pub sla_state: StreamSLAState,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StreamDowntimeReport {
    pub stream_id: u64,
    pub start_time: u64,
    pub end_time: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SignedStreamSLAReport {
    pub report: StreamDowntimeReport,
    pub signature: BytesN<64>,
    pub node_public_key: BytesN<32>,
}

#[contracttype]
#[derive(Clone)]
pub struct TaxReceipt {
    pub meter_id: u64,
    pub total_amount: i128,
    pub tax_amount: i128,
    pub net_amount: i128,
    pub tax_rate_bps: i128,
    pub government_vault: Address,
    pub timestamp: u64,
}

#[contracttype]
pub enum DataKey {
    Meter(u64), Count, Oracle, ActiveMetersCount, SeasonalFactor, Treasury, ProviderVolume(Address),
    SavingGoal(u64), NativeToken, TaxRateBps, ProtocolFeeBps, SupportedToken(Address),
    SupportedWithdrawalToken(Address), ProviderTotalPool(Address), Referral(Address), PollVotes(Symbol),
    UserVoted(Address, Symbol), BillingGroup(Address), WebhookConfig(Address), LastAlert(u64),
    ClosingFeeBps, Contributor(u64, Address), AuthorizedContributor(u64, Address), GovernmentVault,
    MaintenanceWallet, MaintenanceFund(u64), AutoExtendThreshold, ProposedUpgrade, UpgradeProposalTime,
    VetoDeadline, VetoCount, UserVetoed(Address, u64), CurrentAdmin, AdminTransferProposal,
    AdminVeto(Address, u64), ActiveUsers, ComplianceOfficer, ComplianceCouncil, LegalFreeze(u64),
    LegalVault, VerifiedProvider(Address), UserReputation(Address), ReputationMigration(BytesN<32>),
    MigratedReputation(Address, Address), MaintenanceMilestone(u64, u32), ZKProof(BytesN<32>),
    NullifierMap(BytesN<32>), ZKUsageReport(u64, u32), PrivateBillingStatus(u64), CommitmentBatch(u64, u64),
    ZKEnabledMeters, ZKVerificationCache(BytesN<32>), ConservationGoal(u64), GrantStreamMatch(u64, Address),
    SubDaoConfig(Address), MultiSigConfig(Address), WithdrawalRequest(Address, u64),
    WithdrawalRequestCount(Address), WithdrawalApproval(Address, u64, Address), VelocityLimitConfig,
    VelocityOverride(u64), SLANode(BytesN<32>), SLAReportCount((u64, u64, u64)),
    SLAReportNode((u64, u64, u64), BytesN<32>), ContinuousFlow(u64),
    StreamSLAReportCount((u64, u64, u64)), StreamSLAReportNode((u64, u64, u64), BytesN<32>),
    ResellerConfig(u64), ProviderWindow(Address), ImpactSBTMinted(u64), SoroSusuContract,
    NFTMinter, PairingChallenge(u64),
}

#[contracterror]
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
#[repr(u32)]
pub enum ContractError {
    MeterNotFound = 1, OracleNotSet = 2, WithdrawalLimitExceeded = 3, PriceConversionFailed = 4,
    InvalidTokenAmount = 5, InvalidUsageValue = 6, UsageExceedsLimit = 7, InvalidPrecisionFactor = 8,
    InvalidSignature = 9, PublicKeyMismatch = 10, TimestampTooOld = 11, PairingAlreadyComplete = 12,
    ChallengeNotFound = 13, InvalidPairingSignature = 14, MeterNotPaired = 15, MeterPaused = 16,
    AlreadyVoted = 17, InvalidClosingFee = 18, AccountAlreadyClosed = 19, InsufficientBalance = 20,
    UnauthorizedContributor = 21, InDispute = 22, ChallengeActive = 23, NotAnOracle = 24,
    ThrottlingThresholdExceeded = 25, LowPriorityStreamPaused = 26, GovernmentVaultNotSet = 27,
    TaxCalculationFailed = 28, MaintenanceFundInsufficient = 29, TTLExtensionFailed = 30,
    UpgradeProposalActive = 31, VetoPeriodExpired = 32, UserVetoedProposal = 33, InvalidWasmHash = 34,
    AdminTransferActive = 35, NoAdminTransferInProgress = 36, VetoThresholdNotReached = 37,
    AdminExecutionWindowExpired = 38, NotCurrentAdmin = 39, NotComplianceOfficer = 40,
    MeterNotFrozen = 41, LegalFreezeAlreadyActive = 42, ComplianceCouncilApprovalRequired = 43,
    ProviderNotVerified = 44, VerificationAlreadyGranted = 45, NotParentDao = 46,
    SubDaoBudgetExceeded = 47, SubDaoNotConfigured = 48, MultiSigNotConfigured = 49,
    MultiSigAlreadyConfigured = 50, InvalidFinanceWalletCount = 51, InvalidSignatureThreshold = 52,
    NotAuthorizedFinanceWallet = 53, WithdrawalRequestNotFound = 54, WithdrawalRequestExpired = 55,
    WithdrawalAlreadyExecuted = 56, WithdrawalAlreadyCancelled = 57, InsufficientApprovals = 58,
    AlreadyApprovedWithdrawal = 59, NotApprovedByWallet = 60, AmountBelowMultiSigThreshold = 61,
    MultiSigRequiredForAmount = 62, InvalidCommitment = 63, NullifierAlreadyUsed = 64,
    InvalidZKProof = 65, PrivacyNotEnabled = 66, CommitmentNotFound = 67, InvalidBillingCycle = 68,
    ZKVerificationFailed = 69, ConservationGoalNotFound = 70, GoalAlreadyAchieved = 71,
    GoalExpired = 72, InvalidGrantAmount = 73, GrantStreamNotConfigured = 74,
    InsufficientWaterSavings = 75, PerStreamVelocityLimitExceeded = 76, GlobalVelocityLimitExceeded = 77,
    VelocityLimitBreach = 78, NodeNotTrusted = 79, InvalidSLAReport = 80, SLAPenaltyActive = 81,
    InvalidResellerFee = 82, SBTAlreadyMinted = 83, ImpactNotSignificantEnough = 84,
}

#[contractclient(name = "GrantStreamClient")]
pub trait GrantStream {
    fn on_goal_reached(env: Env, goal_event: GoalReachedEvent);
}

#[contract]
pub struct UtilityContract;

// --- Internal Helpers ---

fn get_meter_or_panic(env: &Env, id: u64) -> Meter { env.storage().instance().get(&DataKey::Meter(id)).expect("Meter Not Found") }
fn provider_meter_value(meter: &Meter) -> i128 { meter.balance.max(DEBT_THRESHOLD) }
fn calculate_historical_average(usage_data: &UsageData, now: u64) -> i128 {
    let elapsed = now.saturating_sub(usage_data.first_reading_timestamp);
    if elapsed == 0 { return 0; }
    usage_data.total_watt_hours.saturating_mul(usage_data.precision_factor).saturating_div(elapsed as i128)
}
fn is_peak_hour(timestamp: u64) -> bool {
    let day_seconds = timestamp % DAY_IN_SECONDS;
    day_seconds >= PEAK_HOUR_START && day_seconds <= PEAK_HOUR_END
}
fn get_effective_rate(meter: &Meter, timestamp: u64) -> i128 { if is_peak_hour(timestamp) { meter.peak_rate } else { meter.off_peak_rate } }
fn meter_sla_enabled(config: &SLAConfig) -> bool { config.threshold_seconds > 0 }
fn refresh_activity(meter: &mut Meter, _now: u64) {
    let total_value = match meter.billing_type { BillingType::PrePaid => meter.balance, BillingType::PostPaid => meter.balance.saturating_sub(meter.debt) };
    meter.is_active = total_value > 0 && !meter.is_paused && !meter.is_disputed && !meter.is_closed;
}
fn get_tax_rate_or_default(env: &Env) -> i128 { env.storage().instance().get(&DataKey::TaxRateBps).unwrap_or(DEFAULT_TAX_RATE_BPS) }
fn calculate_tax_split(amount: i128, tax_rate_bps: i128) -> (i128, i128) { let tax_amount = (amount * tax_rate_bps) / 10000; (tax_amount, amount - tax_amount) }
fn get_government_vault_or_default(env: &Env) -> Option<Address> { env.storage().instance().get(&DataKey::GovernmentVault) }
fn allocate_to_maintenance_fund(env: &Env, meter_id: u64, amount: i128) {
    let maintenance_amount = (amount * MAINTENANCE_FUND_PERCENT_BPS) / 10_000;
    if maintenance_amount > 0 {
        let current_fund: i128 = env.storage().instance().get(&DataKey::MaintenanceFund(meter_id)).unwrap_or(0);
        env.storage().instance().set(&DataKey::MaintenanceFund(meter_id), &current_fund.saturating_add(maintenance_amount));
    }
}
fn get_maintenance_fund_balance(env: &Env, meter_id: u64) -> i128 { env.storage().instance().get(&DataKey::MaintenanceFund(meter_id)).unwrap_or(0) }
fn get_reseller_config_impl(env: &Env, meter_id: u64) -> Option<ResellerConfig> { env.storage().instance().get(&DataKey::ResellerConfig(meter_id)) }
fn get_reseller_cut(env: &Env, meter_id: u64, amount: i128) -> i128 { if let Some(config) = get_reseller_config_impl(env, meter_id) { (amount * config.fee_bps) / 10000 } else { 0 } }
fn apply_provider_claim(env: &Env, meter: &mut Meter, amount: i128) {
    if amount <= 0 { return; }
    let client = token::Client::new(env, &meter.token);
    client.transfer(&env.current_contract_address(), &meter.provider, &amount);
    match meter.billing_type { BillingType::PrePaid => { meter.balance = meter.balance.saturating_sub(amount); } BillingType::PostPaid => { meter.debt = meter.debt.saturating_add(amount); } }
    meter.claimed_this_hour = meter.claimed_this_hour.saturating_add(amount);
}
fn get_provider_window_or_default(env: &Env, provider: &Address, now: u64) -> ProviderWithdrawalWindow { env.storage().instance().get(&DataKey::ProviderWindow(provider.clone())).unwrap_or(ProviderWithdrawalWindow { daily_withdrawn: 0, last_reset: now }) }
fn update_provider_total_pool(env: &Env, provider: &Address, old_val: i128, new_val: i128) {
    let current_pool: i128 = env.storage().instance().get(&DataKey::ProviderTotalPool(provider.clone())).unwrap_or(0);
    let updated_pool = current_pool.saturating_sub(old_val).saturating_add(new_val);
    env.storage().instance().set(&DataKey::ProviderTotalPool(provider.clone()), &updated_pool);
}
fn get_provider_total_pool_impl(env: &Env, provider: &Address) -> i128 { env.storage().instance().get(&DataKey::ProviderTotalPool(provider.clone())).unwrap_or(0) }
fn publish_active_event(env: &Env, meter_id: u64, timestamp: u64) { env.events().publish((symbol_short!("Active"), meter_id), timestamp); }
fn auto_extend_ttl_if_needed(env: &Env, meter_id: u64) {
    let threshold: u32 = env.storage().instance().get(&DataKey::AutoExtendThreshold).unwrap_or(AUTO_EXTEND_LEDGER_THRESHOLD);
    if env.ledger().sequence() % threshold == 0 {
        let maintenance_balance = get_maintenance_fund_balance(env, meter_id);
        if maintenance_balance >= 1_000_000 {
            env.storage().instance().set(&DataKey::MaintenanceFund(meter_id), &(maintenance_balance - 1_000_000));
            env.storage().instance().extend_ttl(LEDGER_LIFETIME_EXTENSION, LEDGER_LIFETIME_EXTENSION);
            env.events().publish((symbol_short!("TTLExtnd"), meter_id), (env.ledger().sequence(), LEDGER_LIFETIME_EXTENSION));
        }
    }
}
fn propose_upgrade_impl(env: &Env, new_wasm_hash: BytesN<32>, proposer: &Address) -> u64 {
    let now = env.ledger().timestamp();
    let deadline = now.saturating_add(UPGRADE_VETO_PERIOD_SECONDS);
    env.storage().instance().set(&DataKey::ProposedUpgrade, &UpgradeProposal { new_wasm_hash: new_wasm_hash.clone(), proposed_at: now, veto_deadline: deadline, proposer: proposer.clone() });
    env.storage().instance().set(&DataKey::UpgradeProposalTime, &now);
    env.storage().instance().set(&DataKey::VetoDeadline, &deadline);
    env.events().publish((symbol_short!("UpgrdPrp"),), (new_wasm_hash, now, deadline));
    now
}
fn submit_veto(env: &Env, user: &Address, proposal_id: u64) {
    env.storage().instance().set(&DataKey::UserVetoed(user.clone(), proposal_id), &true);
    let count: i128 = env.storage().instance().get(&DataKey::VetoCount).unwrap_or(0);
    env.storage().instance().set(&DataKey::VetoCount, &(count + 1));
}
fn verify_zk_proof_placeholder(_env: &Env, proof_hash: BytesN<32>) -> bool { for byte in proof_hash.to_array().iter() { if *byte != 0 { return true; } } false }
fn convert_xlm_to_usd_if_needed(_env: &Env, amount: i128, _token: &Address) -> Result<i128, ContractError> { Ok(amount) }
fn convert_usd_to_xlm_if_needed(_env: &Env, usd_cents: i128, _token: &Address) -> Result<i128, ContractError> { Ok(usd_cents) }
fn convert_usd_to_token_if_needed(_env: &Env, usd_cents: i128, _token: &Address) -> Result<i128, ContractError> { Ok(usd_cents) }
fn remaining_postpaid_collateral(meter: &Meter) -> i128 { meter.collateral_limit.saturating_sub(meter.debt) }
fn check_throttling_threshold(_env: &Env, meter: &Meter) -> bool {
    let total = match meter.billing_type { BillingType::PrePaid => meter.balance, BillingType::PostPaid => meter.balance.saturating_sub(meter.debt) };
    total > 0 && meter.balance < (total * THROTTLING_THRESHOLD_PERCENT / 100)
}
fn verify_usage_signature(env: &Env, signed: &SignedUsageData, meter: &Meter) -> Result<(), ContractError> {
    if signed.public_key != meter.device_public_key { return Err(ContractError::PublicKeyMismatch); }
    if env.ledger().timestamp().saturating_sub(signed.timestamp) > MAX_TIMESTAMP_DELAY { return Err(ContractError::TimestampTooOld); }
    #[cfg(not(test))]
    env.crypto().ed25519_verify(&signed.public_key, &UsageReport { meter_id: signed.meter_id, timestamp: signed.timestamp, watt_hours_consumed: signed.watt_hours_consumed, units_consumed: signed.units_consumed, is_renewable_energy: signed.is_renewable_energy }.to_xdr(&env), &signed.signature);
    Ok(())
}

fn get_continuous_flow_or_panic(env: &Env, stream_id: u64) -> ContinuousFlow {
    env.storage()
        .instance()
        .get(&DataKey::ContinuousFlow(stream_id))
        .unwrap_or_else(|| panic_with_error!(env, ContractError::MeterNotFound))
}

fn apply_stream_penalty_if_needed(env: &Env, flow: &mut ContinuousFlow) {
    if flow.sla_state.accumulated_downtime < flow.sla_config.threshold_seconds {
        return;
    }

    let penalized_rate = flow
        .baseline_tokens_per_second
        .saturating_mul(flow.sla_config.penalty_multiplier_bps)
        .saturating_div(10_000);

    if !flow.sla_state.is_penalty_active {
        flow.sla_state.is_penalty_active = true;
        flow.status = ContinuousStreamStatus::Penalized;
        flow.current_tokens_per_second = penalized_rate;
        env.events().publish(
            (Symbol::new(env, "SLAPenaltyApplied"), flow.stream_id),
            (
                flow.sla_state.accumulated_downtime,
                flow.sla_config.penalty_multiplier_bps,
                flow.current_tokens_per_second,
            ),
        );
    } else {
        flow.current_tokens_per_second = penalized_rate;
        flow.status = ContinuousStreamStatus::Penalized;
    }
}

fn restore_stream_baseline(flow: &mut ContinuousFlow) {
    flow.sla_state.accumulated_downtime = 0;
    flow.sla_state.is_penalty_active = false;
    flow.current_tokens_per_second = flow.baseline_tokens_per_second;
    flow.status = ContinuousStreamStatus::Active;
}

fn sync_continuous_flow(env: &Env, flow: &mut ContinuousFlow, now: u64) {
    let elapsed = now.saturating_sub(flow.last_rate_sync_timestamp);
    if elapsed > 0 {
        let accrued = flow
            .current_tokens_per_second
            .saturating_mul(elapsed as i128);
        flow.total_charged = flow.total_charged.saturating_add(accrued);
        flow.last_rate_sync_timestamp = now;
    }

    let stability_window = flow.sla_config.threshold_seconds.saturating_mul(2);
    if flow.sla_state.is_penalty_active
        && now.saturating_sub(flow.sla_state.last_report_timestamp) > stability_window
    {
        restore_stream_baseline(flow);
    }
}

fn settle_claim_for_meter(env: &Env, meter_id: u64, meter: &mut Meter, now: u64) -> ClaimSettlement {
    let elapsed = now.saturating_sub(meter.last_update);
    let mut amount = 0;
    if now.saturating_sub(meter.last_heartbeat) > HEARTBEAT_THRESHOLD_SECONDS {
        if !meter.is_offline { meter.is_offline = true; meter.grace_period_start = meter.last_heartbeat; }
        if now.saturating_sub(meter.grace_period_start) <= GRACE_PERIOD_SECONDS {
            amount = calculate_historical_average(&meter.usage_data, now).saturating_mul(elapsed as i128).saturating_div(meter.usage_data.precision_factor).saturating_mul(get_effective_rate(meter, now));
            meter.estimated_usage_total = meter.estimated_usage_total.saturating_add(amount);
        } else { meter.is_paused = true; }
    } else { amount = (elapsed as i128).saturating_mul(meter.rate_per_unit.saturating_add(meter.credit_drip_rate)); }
    if meter.milestone_deadline > 0 && now > meter.milestone_deadline && !meter.milestone_confirmed { amount /= 2; }
    if meter_sla_enabled(&meter.sla_config) {
        let config = &meter.sla_config;
        if now.saturating_sub(meter.sla_state.last_report_timestamp) > config.threshold_seconds * 2 { meter.sla_state.accumulated_downtime = 0; meter.sla_state.is_penalty_active = false; }
        if meter.sla_state.accumulated_downtime >= config.threshold_seconds {
            if !meter.sla_state.is_penalty_active { meter.sla_state.is_penalty_active = true; env.events().publish((Symbol::new(&env, "SLAPenaltyApplied"), meter_id), (meter.sla_state.accumulated_downtime, config.penalty_multiplier_bps)); }
            amount = amount.saturating_mul(config.penalty_multiplier_bps).saturating_div(10000);
        } else { meter.sla_state.is_penalty_active = false; }
    }
    let claimable = if amount > meter.balance && meter.balance - amount >= DEBT_THRESHOLD { amount } else if amount > meter.balance { meter.balance - DEBT_THRESHOLD } else { amount };
    if claimable <= 0 { return ClaimSettlement { gross_claimed: 0, provider_payout: 0, tax_amount: 0, protocol_fee: 0, reseller_payout: 0 }; }
    let tax = (claimable * get_tax_rate_or_default(env)) / 10000;
    let prot_fee = ((claimable - tax) * env.storage().instance().get::<_, i128>(&DataKey::ProtocolFeeBps).unwrap_or(0)) / 10000;
    let reseller = get_reseller_cut(env, meter_id, claimable - tax - prot_fee);
    meter.balance -= claimable; meter.last_update = now;
    ClaimSettlement { gross_claimed: claimable, provider_payout: claimable - tax - prot_fee - reseller, tax_amount: tax, protocol_fee: prot_fee, reseller_payout: reseller }
}

#[contractimpl]
impl UtilityContract {
    pub fn get_minimum_balance_to_flow() -> i128 { MINIMUM_BALANCE_TO_FLOW }
    pub fn set_oracle(env: Env, addr: Address) { env.storage().instance().set(&DataKey::Oracle, &addr); }
    pub fn set_maintenance_config(env: Env, wallet: Address, fee: i128) { env.storage().instance().set(&DataKey::MaintenanceWallet, &wallet); env.storage().instance().set(&DataKey::ProtocolFeeBps, &fee); }
    pub fn add_supported_token(env: Env, t: Address) { env.storage().instance().set(&DataKey::SupportedToken(t), &true); }
    pub fn register_meter(env: Env, u: Address, p: Address, r: i128, t: Address, pk: BytesN<32>, pr: u32) -> u64 {
        u.require_auth();
        let count = env.storage().instance().get::<_, u64>(&DataKey::Count).unwrap_or(0) + 1;
        let now = env.ledger().timestamp();
        let mut m = Meter { user: u, provider: p, billing_type: BillingType::PrePaid, off_peak_rate: r, peak_rate: r * PEAK_RATE_MULTIPLIER / RATE_PRECISION, rate_per_unit: r, balance: 0, debt: 0, last_update: now, is_active: true, token: t, usage_data: UsageData { total_watt_hours: 0, current_cycle_watt_hours: 0, peak_usage_watt_hours: 0, last_reading_timestamp: now, precision_factor: 1, renewable_watt_hours: 0, renewable_percentage: 0, monthly_volume: 0, last_volume_reset: now, first_reading_timestamp: now }, device_public_key: pk, end_date: 0, rent_deposit: 0, priority_index: pr, green_energy_discount_bps: 0, is_paused: false, is_disputed: false, challenge_timestamp: 0, credit_drip_rate: 0, is_closed: false, off_peak_reward_rate_bps: 0, milestone_deadline: 0, milestone_confirmed: false, rate_per_second: r, collateral_limit: 0, max_flow_rate_per_hour: r * HOUR_IN_SECONDS as i128, last_claim_time: now, claimed_this_hour: 0, is_paired: false, tier_threshold: 100_000, tier_rate: r * 120 / 100, last_heartbeat: now, grace_period_start: 0, is_offline: false, estimated_usage_total: 0, sla_config: SLAConfig { threshold_seconds: 0, penalty_multiplier_bps: 10_000 }, sla_state: SLAState { accumulated_downtime: 0, last_report_timestamp: now, is_penalty_active: false }, parent_account: None, heartbeat: now };
        refresh_activity(&mut m, now);
        env.storage().instance().set(&DataKey::Meter(count), &m);
        env.storage().instance().set(&DataKey::Count, &count);
        count
    }
    pub fn top_up(env: Env, mid: u64, amt: i128, c: Address) {
        let mut m = get_meter_or_panic(&env, mid);
        if c == m.user { c.require_auth(); } else { if !env.storage().instance().get::<_, bool>(&DataKey::AuthorizedContributor(mid, c.clone())).unwrap_or(false) { panic_with_error!(&env, ContractError::UnauthorizedContributor); } c.require_auth(); }
        let old_val = provider_meter_value(&m);
        token::Client::new(&env, &m.token).transfer(&c, &env.current_contract_address(), &amt);
        let conv_amt = convert_xlm_to_usd_if_needed(&env, amt, &m.token).unwrap();
        if conv_amt <= 0 { panic_with_error!(&env, ContractError::InvalidTokenAmount); }
        if m.balance < 0 { let d = conv_amt.min(m.balance.abs()); m.balance += d; m.balance += conv_amt - d; } else { m.balance += conv_amt; }
        let now = env.ledger().timestamp();
        refresh_activity(&mut m, now);
        update_provider_total_pool(&env, &m.provider, old_val, provider_meter_value(&m));
        env.storage().instance().set(&DataKey::Meter(mid), &m);
        env.events().publish((symbol_short!("TokUp"), mid), (amt, conv_amt));
    }
    pub fn claim(env: Env, mid: u64) {
        let mut m = get_meter_or_panic(&env, mid);
        m.provider.require_auth();
        if m.is_disputed { panic_with_error!(&env, ContractError::InDispute); }
        let old_val = provider_meter_value(&m);
        let now = env.ledger().timestamp();
        let s = settle_claim_for_meter(&env, mid, &mut m, now);
        if s.gross_claimed > 0 {
            if let Err(_) = check_velocity_limits(&env, mid, &m.provider, s.gross_claimed) { panic_with_error!(&env, ContractError::VelocityLimitBreach); }
            let tc = token::Client::new(&env, &m.token);
            if s.tax_amount > 0 { if let Some(v) = get_government_vault_or_default(&env) { tc.transfer(&env.current_contract_address(), &v, &s.tax_amount); } }
            if s.protocol_fee > 0 { if let Some(w) = env.storage().instance().get::<_, Address>(&DataKey::MaintenanceWallet) { tc.transfer(&env.current_contract_address(), &w, &s.protocol_fee); } }
            if s.reseller_payout > 0 { if let Some(rc) = get_reseller_config_impl(&env, mid) { tc.transfer(&env.current_contract_address(), &rc.reseller, &s.reseller_payout); } }
            if s.provider_payout > 0 { tc.transfer(&env.current_contract_address(), &m.provider, &s.provider_payout); }
        }
        update_provider_total_pool(&env, &m.provider, old_val, provider_meter_value(&m));
        env.storage().instance().set(&DataKey::Meter(mid), &m);
    }
    pub fn deduct_units(env: Env, sd: SignedUsageData) {
        let mut m = get_meter_or_panic(&env, sd.meter_id);
        m.provider.require_auth();
        verify_usage_signature(&env, &sd, &m).unwrap();
        if m.is_disputed { panic_with_error!(&env, ContractError::InDispute); }
        let old_val = provider_meter_value(&m);
        let now = env.ledger().timestamp();
        let rate = get_effective_rate(&m, sd.timestamp);
        let disc_rate = if sd.is_renewable_energy && m.green_energy_discount_bps > 0 { rate * (10000 - m.green_energy_discount_bps) / 10000 } else { rate };
        if m.is_offline { m.balance += m.estimated_usage_total; m.is_offline = false; m.estimated_usage_total = 0; }
        m.last_heartbeat = now;
        let mut cost = sd.units_consumed * disc_rate;
        if meter_sla_enabled(&m.sla_config) { let config = &m.sla_config; if m.sla_state.is_penalty_active || m.sla_state.accumulated_downtime >= config.threshold_seconds { cost = cost * config.penalty_multiplier_bps / 10000; } }
        allocate_to_maintenance_fund(&env, sd.meter_id, cost);
        let (tax, after_tax) = calculate_tax_split(cost, get_tax_rate_or_default(&env));
        if tax > 0 { if let Some(v) = get_government_vault_or_default(&env) { token::Client::new(&env, &m.token).transfer(&env.current_contract_address(), &v, &tax); } }
        apply_provider_claim(&env, &mut m, after_tax);
        m.usage_data.total_watt_hours += sd.watt_hours_consumed; m.usage_data.current_cycle_watt_hours += sd.watt_hours_consumed;
        if sd.is_renewable_energy { m.usage_data.renewable_watt_hours += sd.watt_hours_consumed; }
        if m.usage_data.total_watt_hours > 0 { m.usage_data.renewable_percentage = m.usage_data.renewable_watt_hours * 10000 / m.usage_data.total_watt_hours; }
        refresh_activity(&mut m, now);
        auto_extend_ttl_if_needed(&env, sd.meter_id);
        update_provider_total_pool(&env, &m.provider, old_val, provider_meter_value(&m));
        env.storage().instance().set(&DataKey::Meter(sd.meter_id), &m);
        env.events().publish((Symbol::new(&env, "UsageReported"), sd.meter_id), (sd.units_consumed, cost));
    }
    pub fn add_sla_node(env: Env, admin: Address, pk: BytesN<32>) { admin.require_auth(); env.storage().instance().set(&DataKey::SLANode(pk), &true); }
    pub fn set_sla_config(env: Env, mid: u64, config: SLAConfig) { let mut m = get_meter_or_panic(&env, mid); m.provider.require_auth(); m.sla_config = config; env.storage().instance().set(&DataKey::Meter(mid), &m); }
    pub fn submit_sla_report(env: Env, sr: SignedSLAReport) {
        if !env.storage().instance().get::<_, bool>(&DataKey::SLANode(sr.node_public_key.clone())).unwrap_or(false) { panic_with_error!(&env, ContractError::NodeNotTrusted); }
        let report_xdr = sr.report.clone().to_xdr(&env);
        env.crypto().ed25519_verify(&sr.node_public_key, &report_xdr, &sr.signature);
        let rk = (sr.report.meter_id, sr.report.start_time, sr.report.end_time);
        if env.storage().temporary().has(&DataKey::SLAReportNode(rk.clone(), sr.node_public_key.clone())) { return; }
        env.storage().temporary().set(&DataKey::SLAReportNode(rk.clone(), sr.node_public_key.clone()), &true);
        let count = env.storage().temporary().get::<_, u32>(&DataKey::SLAReportCount(rk.clone())).unwrap_or(0) + 1;
        env.storage().temporary().set(&DataKey::SLAReportCount(rk.clone()), &count);
        if count == 2 {
            let mut m = get_meter_or_panic(&env, sr.report.meter_id);
            let d = sr.report.end_time.saturating_sub(sr.report.start_time);
            if d > 0 { m.sla_state.accumulated_downtime += d; m.sla_state.last_report_timestamp = env.ledger().timestamp(); env.storage().instance().set(&DataKey::Meter(sr.report.meter_id), &m); }
        }
    }
    pub fn ping(env: Env, mid: u64) { let mut m = get_meter_or_panic(&env, mid); m.provider.require_auth(); m.last_heartbeat = env.ledger().timestamp(); m.is_offline = false; env.storage().instance().set(&DataKey::Meter(mid), &m); }

    pub fn create_continuous_stream(
        env: Env,
        stream_id: u64,
        provider: Address,
        payer: Address,
        tokens_per_second: i128,
        sla_config: StreamSLAConfig,
    ) {
        provider.require_auth();
        payer.require_auth();

        if tokens_per_second <= 0
            || sla_config.threshold_seconds == 0
            || sla_config.penalty_multiplier_bps < 0
            || sla_config.penalty_multiplier_bps > 10_000
        {
            panic_with_error!(&env, ContractError::InvalidTokenAmount);
        }

        let now = env.ledger().timestamp();
        let flow = ContinuousFlow {
            stream_id,
            provider,
            payer,
            baseline_tokens_per_second: tokens_per_second,
            current_tokens_per_second: tokens_per_second,
            total_charged: 0,
            last_rate_sync_timestamp: now,
            created_at: now,
            status: ContinuousStreamStatus::Active,
            sla_config,
            sla_state: StreamSLAState {
                accumulated_downtime: 0,
                last_report_timestamp: now,
                is_penalty_active: false,
            },
        };

        env.storage()
            .instance()
            .set(&DataKey::ContinuousFlow(stream_id), &flow);
    }

    pub fn set_stream_sla_config(env: Env, stream_id: u64, config: StreamSLAConfig) {
        if config.threshold_seconds == 0
            || config.penalty_multiplier_bps < 0
            || config.penalty_multiplier_bps > 10_000
        {
            panic_with_error!(&env, ContractError::InvalidTokenAmount);
        }

        let mut flow = get_continuous_flow_or_panic(&env, stream_id);
        flow.provider.require_auth();

        let now = env.ledger().timestamp();
        sync_continuous_flow(&env, &mut flow, now);
        flow.sla_config = config;

        if flow.sla_state.is_penalty_active {
            apply_stream_penalty_if_needed(&env, &mut flow);
        } else {
            flow.current_tokens_per_second = flow.baseline_tokens_per_second;
            flow.status = ContinuousStreamStatus::Active;
        }

        env.storage()
            .instance()
            .set(&DataKey::ContinuousFlow(stream_id), &flow);
    }

    pub fn submit_stream_sla_report(env: Env, sr: SignedStreamSLAReport) {
        if !env
            .storage()
            .instance()
            .get::<_, bool>(&DataKey::SLANode(sr.node_public_key.clone()))
            .unwrap_or(false)
        {
            panic_with_error!(&env, ContractError::NodeNotTrusted);
        }

        #[cfg(not(test))]
        {
            let report_xdr = sr.report.clone().to_xdr(&env);
            env.crypto()
                .ed25519_verify(&sr.node_public_key, &report_xdr, &sr.signature);
        }

        let report_key = (sr.report.stream_id, sr.report.start_time, sr.report.end_time);
        if env
            .storage()
            .temporary()
            .has(&DataKey::StreamSLAReportNode(report_key.clone(), sr.node_public_key.clone()))
        {
            return;
        }

        env.storage().temporary().set(
            &DataKey::StreamSLAReportNode(report_key.clone(), sr.node_public_key),
            &true,
        );

        let count = env
            .storage()
            .temporary()
            .get::<_, u32>(&DataKey::StreamSLAReportCount(report_key.clone()))
            .unwrap_or(0)
            .saturating_add(1);
        env.storage()
            .temporary()
            .set(&DataKey::StreamSLAReportCount(report_key), &count);

        if count == 2 {
            let mut flow = get_continuous_flow_or_panic(&env, sr.report.stream_id);
            let now = env.ledger().timestamp();
            sync_continuous_flow(&env, &mut flow, now);

            let downtime = sr.report.end_time.saturating_sub(sr.report.start_time);
            if downtime > 0 {
                flow.sla_state.accumulated_downtime = flow
                    .sla_state
                    .accumulated_downtime
                    .saturating_add(downtime);
                flow.sla_state.last_report_timestamp = now;
                apply_stream_penalty_if_needed(&env, &mut flow);
                env.storage()
                    .instance()
                    .set(&DataKey::ContinuousFlow(sr.report.stream_id), &flow);
            }
        }
    }

    pub fn get_continuous_flow(env: Env, stream_id: u64) -> Option<ContinuousFlow> {
        env.storage()
            .instance()
            .get::<_, ContinuousFlow>(&DataKey::ContinuousFlow(stream_id))
            .map(|mut flow| {
                let now = env.ledger().timestamp();
                sync_continuous_flow(&env, &mut flow, now);
                env.storage()
                    .instance()
                    .set(&DataKey::ContinuousFlow(stream_id), &flow);
                flow
            })
    }

    pub fn get_stream_total_charged(env: Env, stream_id: u64) -> i128 {
        let mut flow = get_continuous_flow_or_panic(&env, stream_id);
        let now = env.ledger().timestamp();
        sync_continuous_flow(&env, &mut flow, now);
        env.storage()
            .instance()
            .set(&DataKey::ContinuousFlow(stream_id), &flow);
        flow.total_charged
    }
}
#[cfg(test)]
mod stream_sla_tests;
