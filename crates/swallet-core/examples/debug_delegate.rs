use reqwest::Client;
use serde_json::{json, Value};
use solana_sdk::signer::keypair::Keypair;
use solana_sdk::signer::Signer;

const RPC_URL: &str = "https://devnet-api.nara.build";
const VOTE_ACCOUNT: &str = "8JjRWqAAm7SGY3RWnMieZFjiXRv67CCKGhb7DeAMW5Vy";
// This is the keypair (64 bytes), not the address
const STAKE_KEYPAIR_BS58: &str = "2aP9YsKE7bBPYGDfM2r85WeAi9TU8fGzdXBZ8igtsUeqFnfzm5UTAWViVbERLYe49xxDbtromp6tXPu88Lc3wo8B";

#[tokio::main]
async fn main() {
    let client = Client::new();

    // Decode keypair
    let keypair_bytes = bs58::decode(STAKE_KEYPAIR_BS58).into_vec().unwrap();
    println!("Keypair bytes len: {}", keypair_bytes.len());

    let keypair = Keypair::from_bytes(&keypair_bytes).unwrap();
    let stake_address = keypair.pubkey();
    println!("Stake account address: {stake_address}");

    // Fetch stake account info
    let body = json!({
        "jsonrpc": "2.0",
        "method": "getAccountInfo",
        "params": [stake_address.to_string(), {"encoding": "jsonParsed", "commitment": "confirmed"}],
        "id": 1
    });
    let resp: Value = client.post(RPC_URL).json(&body).send().await.unwrap().json().await.unwrap();

    let value = &resp["result"]["value"];
    if value.is_null() {
        println!("Account does not exist!");
        return;
    }

    let lamports = value["lamports"].as_u64().unwrap_or(0);
    println!("Lamports: {lamports}");
    println!("Owner: {}", value["owner"]);

    let parsed = &value["data"]["parsed"];
    println!("Type: {}", parsed.get("type").unwrap_or(&Value::Null));

    let info = &parsed["info"];
    let meta = &info["meta"];
    println!("Staker: {}", meta["authorized"]["staker"]);
    println!("Withdrawer: {}", meta["authorized"]["withdrawer"]);

    let stake = &info["stake"];
    if !stake.is_null() {
        println!("Delegation voter: {}", stake["delegation"]["voter"]);
        println!("Activation epoch: {}", stake["delegation"]["activationEpoch"]);
        println!("Deactivation epoch: {}", stake["delegation"]["deactivationEpoch"]);
    } else {
        println!("No stake delegation yet (state: initialized)");
    }

    println!("\n--- Attempting delegate ---");

    // Now try to build and send delegate tx
    let stake_pubkey = keypair.pubkey().to_bytes();
    let vote_pubkey = decode_pubkey(VOTE_ACCOUNT).unwrap();
    let clock_sysvar = decode_pubkey("SysvarC1ock11111111111111111111111111111111").unwrap();
    let stake_history = decode_pubkey("SysvarStakeHistory1111111111111111111111111").unwrap();
    let stake_config = decode_pubkey("StakeConfig11111111111111111111111111111111").unwrap();
    let stake_program = decode_pubkey("Stake11111111111111111111111111111111111111").unwrap();

    // DelegateStake instruction (index 2)
    let data = vec![2u8, 0, 0, 0];

    println!("Stake pubkey: {}", keypair.pubkey());
    println!("Vote pubkey: {VOTE_ACCOUNT}");

    // Check if staker authority matches
    let staker = meta["authorized"]["staker"].as_str().unwrap_or("");
    println!("\nStaker authority: {staker}");
    println!("Signing with: {}", keypair.pubkey());
    if staker != keypair.pubkey().to_string() {
        println!("WARNING: Staker authority does NOT match signing key!");
        println!("The delegate tx will fail because we're signing with the wrong key.");
        println!("Staker is: {staker}");
        println!("We're signing with: {}", keypair.pubkey());
        return;
    }

    // Build instruction accounts
    let accounts = vec![
        AccountMeta { pubkey: stake_pubkey, is_signer: false, is_writable: true },
        AccountMeta { pubkey: vote_pubkey, is_signer: false, is_writable: false },
        AccountMeta { pubkey: clock_sysvar, is_signer: false, is_writable: false },
        AccountMeta { pubkey: stake_history, is_signer: false, is_writable: false },
        AccountMeta { pubkey: stake_config, is_signer: false, is_writable: false },
        AccountMeta { pubkey: stake_pubkey, is_signer: true, is_writable: false }, // staker authority
    ];

    let ix = Instruction {
        program_id: stake_program,
        accounts,
        data,
    };

    // Get blockhash
    let bh_body = json!({
        "jsonrpc": "2.0",
        "method": "getLatestBlockhash",
        "params": [{"commitment": "confirmed"}],
        "id": 1
    });
    let bh_resp: Value = client.post(RPC_URL).json(&bh_body).send().await.unwrap().json().await.unwrap();
    let blockhash_str = bh_resp["result"]["value"]["blockhash"].as_str().unwrap();
    println!("\nBlockhash: {blockhash_str}");

    let blockhash_bytes: [u8; 32] = bs58::decode(blockhash_str).into_vec().unwrap().try_into().unwrap();

    // Build message
    let message_bytes = build_and_serialize_message(&stake_pubkey, &blockhash_bytes, &[ix]);

    // Sign
    let sig = keypair.sign_message(&message_bytes);
    let mut sig_bytes = [0u8; 64];
    sig_bytes.copy_from_slice(sig.as_ref());

    // Build tx
    let tx_bytes = build_transaction(&[sig_bytes], &message_bytes);

    // First try simulate
    use base64::Engine;
    let tx_base64 = base64::engine::general_purpose::STANDARD.encode(&tx_bytes);

    let sim_body = json!({
        "jsonrpc": "2.0",
        "method": "simulateTransaction",
        "params": [tx_base64, {"encoding": "base64", "commitment": "confirmed"}],
        "id": 1
    });
    let sim_resp: Value = client.post(RPC_URL).json(&sim_body).send().await.unwrap().json().await.unwrap();
    println!("\nSimulation result:");
    println!("{}", serde_json::to_string_pretty(&sim_resp["result"]["value"]).unwrap());

    let err = &sim_resp["result"]["value"]["err"];
    if err.is_null() {
        println!("\nSimulation succeeded! Sending tx...");
        let send_body = json!({
            "jsonrpc": "2.0",
            "method": "sendTransaction",
            "params": [tx_base64, {"encoding": "base64", "skipPreflight": true}],
            "id": 1
        });
        let send_resp: Value = client.post(RPC_URL).json(&send_body).send().await.unwrap().json().await.unwrap();
        println!("Send result: {}", serde_json::to_string_pretty(&send_resp).unwrap());
    } else {
        println!("\nSimulation FAILED: {}", serde_json::to_string_pretty(err).unwrap());
        // Print logs
        if let Some(logs) = sim_resp["result"]["value"]["logs"].as_array() {
            println!("\nLogs:");
            for log in logs {
                println!("  {}", log.as_str().unwrap_or(""));
            }
        }
    }
}

fn decode_pubkey(s: &str) -> Result<[u8; 32], String> {
    let bytes = bs58::decode(s).into_vec().map_err(|e| format!("{e}"))?;
    bytes.try_into().map_err(|_| "bad len".to_string())
}

// Minimal message building (copied from sol_transfer.rs)
struct AccountKeyMeta {
    pubkey: [u8; 32],
    is_signer: bool,
    is_writable: bool,
    is_fee_payer: bool,
}

struct AccountMeta {
    pubkey: [u8; 32],
    is_signer: bool,
    is_writable: bool,
}

struct Instruction {
    program_id: [u8; 32],
    accounts: Vec<AccountMeta>,
    data: Vec<u8>,
}

struct CompiledInstruction {
    program_id_index: u8,
    account_indices: Vec<u8>,
    data: Vec<u8>,
}

struct Message {
    num_required_signatures: u8,
    num_readonly_signed: u8,
    num_readonly_unsigned: u8,
    account_keys: Vec<[u8; 32]>,
    recent_blockhash: [u8; 32],
    instructions: Vec<CompiledInstruction>,
}

fn add_key(keys: &mut Vec<AccountKeyMeta>, pubkey: &[u8; 32], is_signer: bool, is_writable: bool) {
    if let Some(existing) = keys.iter_mut().find(|k| k.pubkey == *pubkey) {
        existing.is_signer |= is_signer;
        existing.is_writable |= is_writable;
    } else {
        keys.push(AccountKeyMeta {
            pubkey: *pubkey,
            is_signer,
            is_writable,
            is_fee_payer: keys.is_empty(),
        });
    }
}

fn account_sort_order(k: &AccountKeyMeta) -> (u8, u8, u8) {
    if k.is_fee_payer { return (0, 0, 0); }
    match (k.is_signer, k.is_writable) {
        (true, true) => (1, 0, 0),
        (true, false) => (2, 0, 0),
        (false, true) => (3, 0, 0),
        (false, false) => (4, 0, 0),
    }
}

fn build_and_serialize_message(fee_payer: &[u8; 32], recent_blockhash: &[u8; 32], instructions: &[Instruction]) -> Vec<u8> {
    let mut keys: Vec<AccountKeyMeta> = Vec::new();
    add_key(&mut keys, fee_payer, true, true);
    for ix in instructions {
        for meta in &ix.accounts {
            add_key(&mut keys, &meta.pubkey, meta.is_signer, meta.is_writable);
        }
        add_key(&mut keys, &ix.program_id, false, false);
    }
    keys.sort_by_key(account_sort_order);

    let num_readonly_signed = keys.iter().filter(|k| k.is_signer && !k.is_writable).count() as u8;
    let num_readonly_unsigned = keys.iter().filter(|k| !k.is_signer && !k.is_writable).count() as u8;
    let num_signers = keys.iter().filter(|k| k.is_signer).count() as u8;
    let account_keys: Vec<[u8; 32]> = keys.iter().map(|k| k.pubkey).collect();

    println!("\nMessage accounts ({} total, {} signers, {} ro_signed, {} ro_unsigned):",
        account_keys.len(), num_signers, num_readonly_signed, num_readonly_unsigned);
    for (i, k) in keys.iter().enumerate() {
        let flags = format!("{}{}{}",
            if k.is_fee_payer { "F" } else { "" },
            if k.is_signer { "S" } else { "" },
            if k.is_writable { "W" } else { "" },
        );
        println!("  [{}] {} ({})", i, bs58::encode(&k.pubkey).into_string(), flags);
    }

    let compiled: Vec<CompiledInstruction> = instructions.iter().map(|ix| {
        let program_id_index = account_keys.iter().position(|k| k == &ix.program_id).unwrap() as u8;
        let account_indices: Vec<u8> = ix.accounts.iter().map(|m| {
            account_keys.iter().position(|k| k == &m.pubkey).unwrap() as u8
        }).collect();
        CompiledInstruction { program_id_index, account_indices, data: ix.data.clone() }
    }).collect();

    // Serialize
    let mut buf = Vec::new();
    buf.push(num_signers);
    buf.push(num_readonly_signed);
    buf.push(num_readonly_unsigned);
    encode_compact_u16(&mut buf, account_keys.len() as u16);
    for key in &account_keys {
        buf.extend_from_slice(key);
    }
    buf.extend_from_slice(recent_blockhash);
    encode_compact_u16(&mut buf, compiled.len() as u16);
    for ix in &compiled {
        buf.push(ix.program_id_index);
        encode_compact_u16(&mut buf, ix.account_indices.len() as u16);
        buf.extend_from_slice(&ix.account_indices);
        encode_compact_u16(&mut buf, ix.data.len() as u16);
        buf.extend_from_slice(&ix.data);
    }
    buf
}

fn build_transaction(signatures: &[[u8; 64]], message_bytes: &[u8]) -> Vec<u8> {
    let mut buf = Vec::new();
    encode_compact_u16(&mut buf, signatures.len() as u16);
    for sig in signatures {
        buf.extend_from_slice(sig);
    }
    buf.extend_from_slice(message_bytes);
    buf
}

fn encode_compact_u16(buf: &mut Vec<u8>, value: u16) {
    let mut val = value;
    loop {
        let mut byte = (val & 0x7f) as u8;
        val >>= 7;
        if val > 0 { byte |= 0x80; }
        buf.push(byte);
        if val == 0 { break; }
    }
}
