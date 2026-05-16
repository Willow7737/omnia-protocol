// SPDX-License-Identifier: CC0-1.0
pragma solidity ^0.8.19;

/// @title OmniaRollup
/// @notice L1 settlement contract for the Omnia Protocol.
/// @dev Uses Groth16 verification with BN254 (alt_bn128) pairing precompiles.
///      The verifying key is set at construction time and is immutable afterwards.
///
/// ## Proof Format
///
/// The Rust prover (`zk/src/prover.rs`) generates Groth16 proofs using
/// `ark_groth16::Proof<Bn254>` and serializes them with
/// `serialize_uncompressed`. The resulting 256-byte proof is laid out as:
///
/// ```
/// Offset  Length  Field
/// 0       32      A.x          (G1 point, Fp element)
/// 32      32      A.y          (G1 point, Fp element)
/// 64      32      B.x.c0       (G2 point, Fp2 real part)
/// 96      32      B.x.c1       (G2 point, Fp2 imaginary part)
/// 128     32      B.y.c0       (G2 point, Fp2 real part)
/// 160     32      B.y.c1       (G2 point, Fp2 imaginary part)
/// 192     32      C.x          (G1 point, Fp element)
/// 224     32      C.y          (G1 point, Fp element)
/// ```
///
/// When calling `submitBatch`, the proof is passed as structured calldata
/// parameters matching this layout:
///   - `_proofA`: [A.x, A.y]
///   - `_proofB`: [[B.x.c0, B.x.c1], [B.y.c0, B.y.c1]]
///   - `_proofC`: [C.x, C.y]
///
/// ## Verifying Key Format
///
/// The verifying key is also serialized with `serialize_uncompressed` in Rust:
///
/// ```
/// Offset   Length   Field
/// 0        32       alpha.x          (G1)
/// 32       32       alpha.y          (G1)
/// 64       32       beta.x.c0        (G2)
/// 96       32       beta.x.c1        (G2)
/// 128      32       beta.y.c0        (G2)
/// 160      32       beta.y.c1        (G2)
/// 192      32       gamma.x.c0       (G2)
/// 224      32       gamma.x.c1       (G2)
/// 256      32       gamma.y.c0       (G2)
/// 288      32       gamma.y.c1       (G2)
/// 320      32       delta.x.c0       (G2)
/// 352      32       delta.x.c1       (G2)
/// 384      32       delta.y.c0       (G2)
/// 416      32       delta.y.c1       (G2)
/// 448      32       ic_len           (uint256, number of IC elements)
/// 480+     64*ic    IC[i].x || IC[i].y (G1 points, one per public input + 1)
/// ```
///
/// The constructor accepts the VK as flattened arrays:
///   - `_alpha`: [alpha.x, alpha.y]
///   - `_beta`:  [[beta.x.c0, beta.x.c1], [beta.y.c0, beta.y.c1]]
///   - `_gamma`: [[gamma.x.c0, gamma.x.c1], [gamma.y.c0, gamma.y.c1]]
///   - `_delta`: [[delta.x.c0, delta.x.c1], [delta.y.c0, delta.y.c1]]
///   - `_ic`:    [IC[0].x, IC[0].y, IC[1].x, IC[1].y, ...]
///
/// ## Groth16 Verification Equation
///
/// The pairing check verifies:
///   e(A, B) = e(alpha, beta) * e(accum, gamma) * e(C, delta)
///
/// where accum = IC[0] + publicInput[0]*IC[1] + ... + publicInput[n-1]*IC[n]
///
/// This is checked via the EIP-197 pairing precompile as:
///   e(-A, B) * e(alpha, beta) * e(accum, gamma) * e(C, delta) = 1
///
/// ## Public Inputs
///
/// For `RollupCircuit`: 1 public input (expected_new_state_root)
/// For `ExpandedRollupCircuit`: 3 public inputs (old_state_root, new_state_root, event_commitment)
///
/// The number of IC commitments in the verifying key must equal
/// (number of public inputs) + 1.
contract OmniaRollup {
    // -----------------------------------------------------------------------
    // BN254 constants
    // -----------------------------------------------------------------------

    /// @dev Prime field modulus for the BN254 curve (alt_bn128).
    uint256 private constant BN254_FIELD_MODULUS =
        21888242871839275222246405745257275088696311157297823662689037894645226208583;

    /// @dev Precompile address for elliptic curve point addition on G1 (EIP-196).
    address private constant EC_ADD_PRECOMPILE = address(0x06);

    /// @dev Precompile address for elliptic curve scalar multiplication on G1 (EIP-196).
    address private constant EC_MUL_PRECOMPILE = address(0x07);

    /// @dev Precompile address for elliptic curve pairing check (EIP-197).
    address private constant EC_PAIRING_PRECOMPILE = address(0x08);

    // -----------------------------------------------------------------------
    // Structs
    // -----------------------------------------------------------------------

    /// @notice A point on the G1 subgroup of the BN254 curve.
    struct G1Point {
        uint256 x;
        uint256 y;
    }

    /// @notice A point on the G2 subgroup of the BN254 curve.
    /// @dev Each coordinate is an Fp2 element: c0 + c1*i, where i^2 + 1 = 0.
    ///      x[0] = c0 (real part), x[1] = c1 (imaginary part).
    struct G2Point {
        uint256[2] x;
        uint256[2] y;
    }

    // -----------------------------------------------------------------------
    // State variables
    // -----------------------------------------------------------------------

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

    // -----------------------------------------------------------------------
    // Verifying key (set in constructor, immutable afterwards)
    // -----------------------------------------------------------------------

    /// @notice Alpha G1 point of the verifying key.
    G1Point public vkAlpha;

    /// @notice Beta G2 point of the verifying key.
    G2Point public vkBeta;

    /// @notice Gamma G2 point of the verifying key.
    G2Point public vkGamma;

    /// @notice Delta G2 point of the verifying key.
    G2Point public vkDelta;

    /// @notice IC (gamma-ABC) G1 commitments. Length = numPublicInputs + 1.
    ///         IC[0] is the constant term; IC[i] corresponds to public input i-1.
    G1Point[] public vkIC;

    // -----------------------------------------------------------------------
    // Events
    // -----------------------------------------------------------------------

    event StateUpdated(bytes32 indexed oldRoot, bytes32 indexed newRoot, uint256 batchIndex);
    event Deposited(address indexed sender, bytes32 indexed l2Did, uint256 amount);
    event WithdrawalRequested(address indexed recipient, bytes32 indexed l2Did, uint256 amount);
    event WithdrawalFinalized(address indexed recipient, uint256 amount);

    // -----------------------------------------------------------------------
    // Constructor
    // -----------------------------------------------------------------------

    /// @notice Deploy the rollup contract with the given operator, initial state
    ///         root, and Groth16 verifying key.
    /// @param _operator Address authorized to submit batches.
    /// @param _initialStateRoot Initial L2 state root.
    /// @param _alpha Verifying key alpha point (G1): [x, y].
    /// @param _beta Verifying key beta point (G2): [[x.c0, x.c1], [y.c0, y.c1]].
    /// @param _gamma Verifying key gamma point (G2): [[x.c0, x.c1], [y.c0, y.c1]].
    /// @param _delta Verifying key delta point (G2): [[x.c0, x.c1], [y.c0, y.c1]].
    /// @param _ic Flattened IC commitments: [IC0.x, IC0.y, IC1.x, IC1.y, ...].
    ///        Length must be even (pairs of x, y coordinates).
    constructor(
        address _operator,
        bytes32 _initialStateRoot,
        uint256[2] memory _alpha,
        uint256[2][2] memory _beta,
        uint256[2][2] memory _gamma,
        uint256[2][2] memory _delta,
        uint256[] memory _ic
    ) {
        require(_operator != address(0), "Zero operator");
        operator = _operator;
        stateRoot = _initialStateRoot;

        // Store verifying key
        vkAlpha = G1Point(_alpha[0], _alpha[1]);
        vkBeta = G2Point([_beta[0][0], _beta[0][1]], [_beta[1][0], _beta[1][1]]);
        vkGamma = G2Point([_gamma[0][0], _gamma[0][1]], [_gamma[1][0], _gamma[1][1]]);
        vkDelta = G2Point([_delta[0][0], _delta[0][1]], [_delta[1][0], _delta[1][1]]);

        require(_ic.length % 2 == 0, "IC length must be even");
        for (uint256 i = 0; i < _ic.length; i += 2) {
            vkIC.push(G1Point(_ic[i], _ic[i + 1]));
        }
    }

    // -----------------------------------------------------------------------
    // Batch submission
    // -----------------------------------------------------------------------

    /// @notice Submit a new batch with a Groth16 ZK proof.
    /// @param _newStateRoot The new L2 state root after the batch.
    /// @param _proofA Groth16 proof point A (G1): [x, y].
    /// @param _proofB Groth16 proof point B (G2): [[x.c0, x.c1], [y.c0, y.c1]].
    ///        Convention matches arkworks: c0 = real part, c1 = imaginary part.
    /// @param _proofC Groth16 proof point C (G1): [x, y].
    /// @param _publicInputs Circuit public inputs.
    ///        RollupCircuit: [expected_new_state_root] (1 input).
    ///        ExpandedRollupCircuit: [old_state_root, new_state_root, event_commitment] (3 inputs).
    /// @param _batchData Arbitrary batch data (posted as calldata for DA).
    function submitBatch(
        bytes32 _newStateRoot,
        uint256[2] calldata _proofA,
        uint256[2][2] calldata _proofB,
        uint256[2] calldata _proofC,
        uint256[] calldata _publicInputs,
        bytes calldata _batchData
    ) external onlyOperator {
        require(verifyProof(_proofA, _proofB, _proofC, _publicInputs), "Invalid proof");
        bytes32 oldRoot = stateRoot;
        stateRoot = _newStateRoot;
        emit StateUpdated(oldRoot, _newStateRoot, batchIndex++);
    }

    // -----------------------------------------------------------------------
    // Groth16 Verification
    // -----------------------------------------------------------------------

    /// @notice Verify a Groth16 proof against the stored verifying key.
    /// @dev Implements the pairing check:
    ///      e(-A, B) * e(alpha, beta) * e(accum, gamma) * e(C, delta) = 1
    ///      where accum = IC[0] + sum(publicInput[i] * IC[i+1]).
    ///
    ///      Uses EIP-197 pairing precompile (0x08) for the pairing check,
    ///      EIP-196 ecMul precompile (0x07) for scalar multiplication, and
    ///      EIP-196 ecAdd precompile (0x06) for point addition.
    ///
    /// @param _a Proof point A (G1): [x, y].
    /// @param _b Proof point B (G2): [[x.c0, x.c1], [y.c0, y.c1]].
    /// @param _c Proof point C (G1): [x, y].
    /// @param _publicInputs Circuit public inputs.
    /// @return valid True if the proof is valid.
    function verifyProof(
        uint256[2] memory _a,
        uint256[2][2] memory _b,
        uint256[2] memory _c,
        uint256[] memory _publicInputs
    ) internal view returns (bool valid) {
        // Validate public inputs length matches verifying key
        require(_publicInputs.length + 1 == vkIC.length, "Public input count mismatch");

        // ------------------------------------------------------------------
        // Step 1: Compute the linear combination accumulator
        //   accum = IC[0] + publicInput[0]*IC[1] + ... + publicInput[n-1]*IC[n]
        // ------------------------------------------------------------------
        G1Point memory accum = vkIC[0];
        for (uint256 i = 0; i < _publicInputs.length; i++) {
            G1Point memory scaled = _ecMul(vkIC[i + 1], _publicInputs[i]);
            accum = _ecAdd(accum, scaled);
        }

        // ------------------------------------------------------------------
        // Step 2: Prepare the pairing check
        //
        // The Groth16 verification equation is:
        //   e(A, B) = e(alpha, beta) * e(accum, gamma) * e(C, delta)
        //
        // Rearranging for the pairing precompile (which checks product = 1):
        //   e(-A, B) * e(alpha, beta) * e(accum, gamma) * e(C, delta) = 1
        //
        // The pairing precompile input is a sequence of (G1, G2) pairs:
        //   [neg(A), B, alpha, beta, accum, gamma, C, delta]
        //
        // Each pair is encoded as:
        //   G1: 64 bytes (x || y), each coordinate as 32-byte big-endian uint256
        //   G2: 128 bytes, encoded per EIP-197 as:
        //       x_c1 || x_c0 || y_c1 || y_c0
        //       (imaginary part first, then real part)
        // ------------------------------------------------------------------

        // Negate A: (x, y) -> (x, p - y) where p is the BN254 field modulus
        uint256 negAY = BN254_FIELD_MODULUS - (_a[1] % BN254_FIELD_MODULUS);

        // Build the 768-byte input for the pairing precompile (4 pairs x 192 bytes).
        // We use a fixed-size uint256[24] array (24 * 32 = 768 bytes).
        uint256[24] memory pairingInput;

        // --- Pair 1: (-A, B) ---
        pairingInput[0]  = _a[0];         // -A.x
        pairingInput[1]  = negAY;         // -A.y
        // EIP-197 G2 encoding: x_c1, x_c0, y_c1, y_c0
        pairingInput[2]  = _b[0][1];      // B.x.c1 (imaginary)
        pairingInput[3]  = _b[0][0];      // B.x.c0 (real)
        pairingInput[4]  = _b[1][1];      // B.y.c1 (imaginary)
        pairingInput[5]  = _b[1][0];      // B.y.c0 (real)

        // --- Pair 2: (alpha, beta) ---
        pairingInput[6]  = vkAlpha.x;         // alpha.x
        pairingInput[7]  = vkAlpha.y;         // alpha.y
        pairingInput[8]  = vkBeta.x[1];       // beta.x.c1
        pairingInput[9]  = vkBeta.x[0];       // beta.x.c0
        pairingInput[10] = vkBeta.y[1];       // beta.y.c1
        pairingInput[11] = vkBeta.y[0];       // beta.y.c0

        // --- Pair 3: (accum, gamma) ---
        pairingInput[12] = accum.x;           // accum.x
        pairingInput[13] = accum.y;           // accum.y
        pairingInput[14] = vkGamma.x[1];      // gamma.x.c1
        pairingInput[15] = vkGamma.x[0];      // gamma.x.c0
        pairingInput[16] = vkGamma.y[1];      // gamma.y.c1
        pairingInput[17] = vkGamma.y[0];      // gamma.y.c0

        // --- Pair 4: (C, delta) ---
        pairingInput[18] = _c[0];         // C.x
        pairingInput[19] = _c[1];         // C.y
        pairingInput[20] = vkDelta.x[1];      // delta.x.c1
        pairingInput[21] = vkDelta.x[0];      // delta.x.c0
        pairingInput[22] = vkDelta.y[1];      // delta.y.c1
        pairingInput[23] = vkDelta.y[0];      // delta.y.c0

        // ------------------------------------------------------------------
        // Step 3: Call the pairing precompile
        // ------------------------------------------------------------------
        uint256[1] memory result;
        bool success;
        assembly {
            success := staticcall(
                sub(gas(), 2000),               // retain 2000 gas for cleanup
                EC_PAIRING_PRECOMPILE,           // 0x08
                pairingInput,                    // input pointer
                768,                             // input size: 4 pairs * 192 bytes
                result,                          // output pointer
                32                               // output size: 32 bytes
            )
        }
        require(success, "Pairing precompile call failed");

        // The precompile returns 1 (as 32 bytes) if the pairing product equals 1.
        return result[0] == 1;
    }

    // -----------------------------------------------------------------------
    // BN254 Precompile Wrappers
    // -----------------------------------------------------------------------

    /// @notice Add two G1 points using the ecAdd precompile (0x06).
    /// @param p1 First G1 point.
    /// @param p2 Second G1 point.
    /// @return r The sum p1 + p2 as a G1 point.
    function _ecAdd(G1Point memory p1, G1Point memory p2) internal view returns (G1Point memory r) {
        // Encode: p1.x || p1.y || p2.x || p2.y = 128 bytes
        uint256[4] memory input;
        input[0] = p1.x;
        input[1] = p1.y;
        input[2] = p2.x;
        input[3] = p2.y;

        uint256[2] memory output;
        bool success;
        assembly {
            success := staticcall(
                sub(gas(), 2000),
                EC_ADD_PRECOMPILE,   // 0x06
                input,
                128,                 // 4 * 32 bytes
                output,
                64                   // 2 * 32 bytes
            )
        }
        require(success, "ecAdd precompile call failed");

        r.x = output[0];
        r.y = output[1];
    }

    /// @notice Scalar multiplication on G1 using the ecMul precompile (0x07).
    /// @param p The G1 point.
    /// @param s The scalar (field element).
    /// @return r The product s * p as a G1 point.
    function _ecMul(G1Point memory p, uint256 s) internal view returns (G1Point memory r) {
        // Encode: p.x || p.y || s = 96 bytes
        uint256[3] memory input;
        input[0] = p.x;
        input[1] = p.y;
        input[2] = s;

        uint256[2] memory output;
        bool success;
        assembly {
            success := staticcall(
                sub(gas(), 2000),
                EC_MUL_PRECOMPILE,   // 0x07
                input,
                96,                  // 3 * 32 bytes
                output,
                64                   // 2 * 32 bytes
            )
        }
        require(success, "ecMul precompile call failed");

        r.x = output[0];
        r.y = output[1];
    }

    // -----------------------------------------------------------------------
    // Deposit & Withdrawal
    // -----------------------------------------------------------------------

    /// @notice Deposit ETH into the rollup, credited to an L2 identity.
    function deposit(bytes32 _l2Did) external payable {
        require(msg.value > 0, "Must deposit > 0");
        deposits[_l2Did] += msg.value;
        emit Deposited(msg.sender, _l2Did, msg.value);
    }

    /// @notice Request a withdrawal from L2 to L1.
    function requestWithdrawal(bytes32 _l2Did, uint256 _amount) external {
        require(deposits[_l2Did] >= _amount, "Insufficient balance");
        withdrawals[msg.sender].push(Withdrawal({
            l2Did: _l2Did,
            amount: _amount,
            requestedAt: block.timestamp
        }));
        emit WithdrawalRequested(msg.sender, _l2Did, _amount);
    }

    /// @notice Finalize a withdrawal after the 7-day challenge period.
    /// @dev Follows the Checks-Effects-Interactions pattern to prevent reentrancy.
    ///      State is cleared BEFORE the external ETH transfer.
    function finalizeWithdrawal(uint256 _withdrawalIndex) external {
        Withdrawal storage w = withdrawals[msg.sender][_withdrawalIndex];
        require(w.amount > 0, "Invalid withdrawal");
        require(block.timestamp >= w.requestedAt + 7 days, "Challenge period active");

        // --- Effects: save and clear state BEFORE external call ---
        bytes32 l2Did = w.l2Did;
        uint256 amount = w.amount;

        // Zero out the withdrawal to prevent reentrancy
        w.l2Did = bytes32(0);
        w.amount = 0;
        w.requestedAt = 0;

        // Decrease the deposit balance
        deposits[l2Did] -= amount;

        // --- Interactions: external call LAST ---
        payable(msg.sender).transfer(amount);

        emit WithdrawalFinalized(msg.sender, amount);
    }

    // -----------------------------------------------------------------------
    // Modifiers
    // -----------------------------------------------------------------------

    modifier onlyOperator() {
        require(msg.sender == operator, "Not operator");
        _;
    }

    // -----------------------------------------------------------------------
    // View helpers
    // -----------------------------------------------------------------------

    /// @notice Returns the number of IC commitments in the verifying key.
    ///         This equals (number of public inputs) + 1.
    function vkICLength() external view returns (uint256) {
        return vkIC.length;
    }
}
