use data_indexing_caching::parser::create_erc20_abi;

#[test]
fn test_parser_abi_creation() {
    let abi = create_erc20_abi();
    // Verify it contains standard ERC20 events
    assert!(format!("{:?}", abi).contains("Transfer"));
    assert!(format!("{:?}", abi).contains("Approval"));
}
