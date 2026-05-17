#![allow(clippy::unwrap_used)]
use omnia_substrate::*;
use proptest::prelude::*;

proptest! {
    #[test]
    fn test_gcounter_commutativity(a in 0u64..10000, b in 0u64..10000, c in 0u64..10000) {
        let mut g1 = GCounter::new();
        let mut g2 = GCounter::new();

        let mut node_a = [0u8; 32]; node_a[0] = 1;
        let mut node_b = [0u8; 32]; node_b[0] = 2;
        let mut node_c = [0u8; 32]; node_c[0] = 3;

        g1.increment(node_a, a);
        g1.increment(node_b, b);
        g1.increment(node_c, c);

        // Different order
        g2.increment(node_c, c);
        g2.increment(node_a, a);
        g2.increment(node_b, b);

        prop_assert_eq!(g1.value(), g2.value());
    }

    #[test]
    fn test_gcounter_merge_associativity(
        a in 0u64..1000, b in 0u64..1000, c in 0u64..1000
    ) {
        let mut node_a = [0u8; 32]; node_a[0] = 1;
        let mut node_b = [0u8; 32]; node_b[0] = 2;
        let mut node_c = [0u8; 32]; node_c[0] = 3;

        let mut g1 = GCounter::new();
        g1.increment(node_a, a);

        let mut g2 = GCounter::new();
        g2.increment(node_b, b);

        let mut g3 = GCounter::new();
        g3.increment(node_c, c);

        // (g1 merge g2) merge g3 == g1 merge (g2 merge g3)
        let mut left = g1.clone();
        left.merge(&g2);
        left.merge(&g3);

        let mut right_mid = g2.clone();
        right_mid.merge(&g3);
        let mut right = g1.clone();
        right.merge(&right_mid);

        prop_assert_eq!(left.value(), right.value());
    }

    #[test]
    fn test_vector_clock_partial_order(
        a in 0u64..100, b in 0u64..100, c in 0u64..100
    ) {
        let mut node_x = [0u8; 32]; node_x[0] = 1;
        let mut node_y = [0u8; 32]; node_y[0] = 2;

        let mut vc_a = VectorClock::new();
        vc_a.set(node_x, a);
        vc_a.set(node_y, b);

        let mut vc_b = VectorClock::new();
        vc_b.set(node_x, a);
        vc_b.set(node_y, c);

        let order = vc_a.compare(&vc_b);
        if b == c {
            prop_assert_eq!(order, CausalOrder::Equal);
        } else if b < c {
            prop_assert_eq!(order, CausalOrder::Before);
        } else {
            prop_assert_eq!(order, CausalOrder::After);
        }
    }
}
