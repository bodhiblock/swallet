//! Optional integration check against a local validator with Squads and its ProgramConfig cloned.
//! SWALLET_LOCAL_RPC=http://127.0.0.1:18899 cargo test -p swallet-core --lib local_spl_flow -- --ignored --nocapture
use super::super::token;
use super::*;

async fn confirmed(client: &Client, rpc: &str, signature: &str) {
    for _ in 0..300 {
        let response = sol_transfer::rpc_call(client, rpc, &json!({"jsonrpc":"2.0", "id":1,
            "method":"getSignatureStatuses", "params":[[signature], {"searchTransactionHistory":true}]})).await.unwrap();
        assert!(response["error"].is_null(), "{response}");
        let status = &response["result"]["value"][0];
        if !status.is_null() {
            assert!(status["err"].is_null(), "{status}");
            if matches!(
                status["confirmationStatus"].as_str(),
                Some("confirmed" | "finalized")
            ) {
                return;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    panic!("Local transaction did not confirm: {signature}");
}

async fn send(
    client: &Client,
    rpc: &str,
    payer: &Keypair,
    extra: &[&Keypair],
    instructions: Vec<solana_sdk::instruction::Instruction>,
) {
    let instructions: Vec<Instruction> = instructions
        .into_iter()
        .map(|ix| Instruction {
            program_id: ix.program_id.to_bytes(),
            data: ix.data,
            accounts: ix
                .accounts
                .into_iter()
                .map(|a| AccountMeta {
                    pubkey: a.pubkey.to_bytes(),
                    is_signer: a.is_signer,
                    is_writable: a.is_writable,
                })
                .collect(),
        })
        .collect();
    let hash = sol_transfer::get_latest_blockhash(client, rpc)
        .await
        .unwrap();
    let message =
        sol_transfer::build_and_serialize_message(&payer.pubkey().to_bytes(), &hash, &instructions);
    let signatures: Vec<[u8; 64]> = std::iter::once(payer)
        .chain(extra.iter().copied())
        .map(|k| k.sign_message(&message).as_ref().try_into().unwrap())
        .collect();
    let bytes = sol_transfer::build_transaction(&signatures, &message);
    let sig = sol_transfer::send_transaction(client, rpc, &bytes)
        .await
        .unwrap();
    confirmed(client, rpc, &sig).await;
}

fn sdk(ix: proposals::VaultInstruction) -> solana_sdk::instruction::Instruction {
    solana_sdk::instruction::Instruction {
        program_id: Pubkey::new_from_array(ix.program_id),
        data: ix.data,
        accounts: ix
            .accounts
            .into_iter()
            .map(|a| solana_sdk::instruction::AccountMeta {
                pubkey: Pubkey::new_from_array(a.pubkey),
                is_signer: a.is_signer,
                is_writable: a.is_writable,
            })
            .collect(),
    }
}

#[tokio::test]
#[ignore = "requires a local Solana validator with Squads and its ProgramConfig"]
async fn local_spl_flow() {
    let rpc =
        std::env::var("SWALLET_LOCAL_RPC").expect("Set SWALLET_LOCAL_RPC to a loopback validator");
    let url = reqwest::Url::parse(&rpc).unwrap();
    assert_eq!(
        url.host_str(),
        Some("127.0.0.1"),
        "This test must never use a real cluster"
    );
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .unwrap();
    let payer = Keypair::new();
    let approver = Keypair::new();
    let secret = payer.to_bytes();
    let approver_secret = approver.to_bytes();
    let airdrop = sol_transfer::rpc_call(
        &client,
        &rpc,
        &json!({"jsonrpc":"2.0", "id":1,
        "method":"requestAirdrop", "params":[payer.pubkey().to_string(), 100_000_000_000u64]}),
    )
    .await
    .unwrap();
    confirmed(&client, &rpc, airdrop["result"].as_str().unwrap()).await;
    let mut members = [payer.pubkey(), approver.pubkey()];
    members.sort();
    let created = create_multisig_v2(&client, &rpc, &secret[..32], &members, 2, None)
        .await
        .unwrap();
    let (address, sig) = created.split_once('|').unwrap();
    confirmed(&client, &rpc, sig).await;
    let multisig: Pubkey = address.parse().unwrap();
    let (vault, _) = derive_vault_pda(&multisig, 3);
    let sig = sol_transfer::send_sol_native(
        &client,
        &rpc,
        &secret[..32],
        &vault.to_string(),
        1_000_000_000,
    )
    .await
    .unwrap();
    confirmed(&client, &rpc, &sig).await;
    for pid in [token::TOKEN, token::TOKEN_2022] {
        let pid: Pubkey = pid.parse().unwrap();
        let mint = Keypair::new();
        let to = Pubkey::new_unique();
        let config = crate::config::SplTokenConfig {
            mint_address: mint.pubkey().to_string(),
            symbol: "TEST".into(),
            decimals: 6,
            is_token_2022: pid.to_string() == token::TOKEN_2022,
        };
        let source: Pubkey = crate::chain::solana::derive_ata_address(
            &vault.to_string(),
            &config.mint_address,
            config.is_token_2022,
        )
        .unwrap()
        .parse()
        .unwrap();
        let dest: Pubkey = crate::chain::solana::derive_ata_address(
            &to.to_string(),
            &config.mint_address,
            config.is_token_2022,
        )
        .unwrap()
        .parse()
        .unwrap();
        let rent = sol_transfer::rpc_call(
            &client,
            &rpc,
            &json!({"jsonrpc":"2.0", "id":1,
            "method":"getMinimumBalanceForRentExemption", "params":[82]}),
        )
        .await
        .unwrap()["result"]
            .as_u64()
            .unwrap();
        let create = solana_sdk::system_instruction::create_account(
            &payer.pubkey(),
            &mint.pubkey(),
            rent,
            82,
            &pid,
        );
        let mut initialize = spl_token::instruction::initialize_mint2(
            &spl_token::id(),
            &mint.pubkey(),
            &payer.pubkey(),
            None,
            6,
        )
        .unwrap();
        initialize.program_id = pid;
        send(&client, &rpc, &payer, &[&mint], vec![create, initialize]).await;
        let create_ata = |owner: &Pubkey, ata: &Pubkey| {
            sdk(proposals::build_spl_transfer_instructions(
                &payer.pubkey().to_bytes(),
                &owner.to_bytes(),
                &mint.pubkey().to_bytes(),
                1,
                6,
                &pid.to_bytes(),
                &source.to_bytes(),
                &ata.to_bytes(),
                true,
            )
            .remove(0))
        };
        let mut mint_to = spl_token::instruction::mint_to(
            &spl_token::id(),
            &mint.pubkey(),
            &source,
            &payer.pubkey(),
            &[],
            10_000_000,
        )
        .unwrap();
        mint_to.program_id = pid;
        send(
            &client,
            &rpc,
            &payer,
            &[],
            vec![create_ata(&vault, &source), mint_to],
        )
        .await;
        let instructions =
            token::prepare_transfer(&client, &rpc, &vault, &to.to_string(), "1.23", &config)
                .await
                .unwrap();
        let sig = create_proposal_and_approve(
            &client,
            &rpc,
            &secret[..32],
            &secret[..32],
            address,
            3,
            instructions,
        )
        .await
        .unwrap();
        confirmed(&client, &rpc, &sig).await;
        let info = fetch_multisig(&client, &rpc, address).await.unwrap();
        let proposals = fetch_active_proposals(&client, &rpc, &info, "SOL", &[config])
            .await
            .unwrap();
        let proposal = &proposals[0];
        assert_eq!(proposal.vault_index, Some(3));
        assert_eq!(proposal.review_error, None, "{:?}", proposal.summary);
        assert!(proposal.summary.as_ref().unwrap().contains("1.23 TEST"));
        assert_eq!(proposal.approved, vec![payer.pubkey()]);
        assert_eq!(proposal.status, super::super::ProposalStatus::Active);
        let sig = approve_proposal(
            &client,
            &rpc,
            &approver_secret[..32],
            &secret[..32],
            address,
            info.transaction_index,
        )
        .await
        .unwrap();
        confirmed(&client, &rpc, &sig).await;
        // Simulate another sender creating the destination ATA while approval is pending.
        send(&client, &rpc, &payer, &[], vec![create_ata(&to, &dest)]).await;
        // Deliberately pass the wrong menu vault; execution must use the on-chain vault index.
        let sig = execute_vault_transaction(
            &client,
            &rpc,
            &secret[..32],
            &secret[..32],
            address,
            info.transaction_index,
            0,
        )
        .await
        .unwrap();
        confirmed(&client, &rpc, &sig).await;
        let accounts =
            token::fetch_accounts(&client, &rpc, &[source.to_string(), dest.to_string()])
                .await
                .unwrap();
        assert_eq!(
            accounts[0]["data"]["parsed"]["info"]["tokenAmount"]["amount"],
            "8770000"
        );
        assert_eq!(
            accounts[1]["data"]["parsed"]["info"]["tokenAmount"]["amount"],
            "1230000"
        );
        assert_eq!(
            fetch_proposal(
                &client,
                &rpc,
                &derive_proposal_pda(&multisig, info.transaction_index)
                    .0
                    .to_string()
            )
            .await
            .unwrap()
            .status,
            super::super::ProposalStatus::Executed
        );
        println!(
            "{}: create → review → approve → execute, balances verified",
            pid
        );
    }
}
