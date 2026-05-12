// SPDX-License-Identifier: CC0-1.0
pragma solidity ^0.8.19;

/// @title OmniaRollup
/// @notice L1 settlement contract for the Omnia Protocol.
/// @dev Phase 0: Proof verification is a stub. Production will use
///      a Groth16 verifier with a pre-compiled verifying key.
contract OmniaRollup {
    bytes32 public stateRoot;
    address public operator;
    uint256 public batchIndex;

    mapping(bytes32 => uint256) public deposits;

    struct Withdrawal {
        bytes32 l2Did;
        uint256 amount;
        uint256 requestedAt;
    }
    mapping(address => Withdrawal[]) public withdrawals;

    event StateUpdated(bytes32 indexed oldRoot, bytes32 indexed newRoot, uint256 batchIndex);
    event Deposited(address indexed sender, bytes32 indexed l2Did, uint256 amount);
    event WithdrawalRequested(address indexed recipient, bytes32 indexed l2Did, uint256 amount);
    event WithdrawalFinalized(address indexed recipient, uint256 amount);

    constructor(address _operator, bytes32 _initialStateRoot) {
        operator = _operator;
        stateRoot = _initialStateRoot;
    }

    /// @notice Submit a new batch with ZK proof.
    /// @dev Phase 0: proof verification is a stub (checks non-empty).
    function submitBatch(
        bytes32 _newStateRoot,
        bytes calldata _proof,
        bytes calldata _batchData
    ) external onlyOperator {
        require(verifyProof(_proof), "Invalid proof");
        bytes32 oldRoot = stateRoot;
        stateRoot = _newStateRoot;
        emit StateUpdated(oldRoot, _newStateRoot, batchIndex++);
    }

    /// @notice Phase 0 stub: checks proof is non-empty.
    function verifyProof(bytes calldata _proof) internal pure returns (bool) {
        return _proof.length > 0;
    }

    function deposit(bytes32 _l2Did) external payable {
        require(msg.value > 0, "Must deposit > 0");
        deposits[_l2Did] += msg.value;
        emit Deposited(msg.sender, _l2Did, msg.value);
    }

    function requestWithdrawal(bytes32 _l2Did, uint256 _amount) external {
        require(deposits[_l2Did] >= _amount, "Insufficient balance");
        withdrawals[msg.sender].push(Withdrawal({
            l2Did: _l2Did,
            amount: _amount,
            requestedAt: block.timestamp
        }));
        emit WithdrawalRequested(msg.sender, _l2Did, _amount);
    }

    function finalizeWithdrawal(uint256 _withdrawalIndex) external {
        Withdrawal storage w = withdrawals[msg.sender][_withdrawalIndex];
        require(w.amount > 0, "Invalid withdrawal");
        require(block.timestamp >= w.requestedAt + 7 days, "Challenge period active");
        deposits[w.l2Did] -= w.amount;
        payable(msg.sender).transfer(w.amount);
        emit WithdrawalFinalized(msg.sender, w.amount);
    }

    modifier onlyOperator() {
        require(msg.sender == operator, "Not operator");
        _;
    }
}
