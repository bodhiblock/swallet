anchor_lang::declare_program!(squads_multisig_program);
anchor_lang::declare_program!(nara_quest);
anchor_lang::declare_program!(nara_agent_registry);
anchor_lang::declare_program!(nara_skills_hub);
anchor_lang::declare_program!(nara_zk);

pub mod error;
pub mod config;
pub mod crypto;
pub mod storage;
pub mod chain;
pub mod transfer;
pub mod multisig;
pub mod staking;
