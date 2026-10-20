use proptest::prelude::*;
use proptest::test_runner::TestRunner;

fn word() -> impl Strategy<Value = u32> + Clone {
    prop_oneof![any::<u32>(), any::<u32>(), any::<u32>()]
}

fn entry() -> impl Strategy<Value = Option<(u32, u32, u32)>> + Clone {
    prop::option::of((word(), word(), word()))
}

fn deeply_nested() -> impl Strategy<Value = [[Option<(u32, u32, u32)>; 4]; 4]> {
    any::<bool>().prop_flat_map(|_| {
        prop::array::uniform4(prop::array::uniform4(entry()))
    })
}

fn new_tree_on_stack(stack_size: usize) -> usize {
    std::thread::Builder::new()
        .stack_size(stack_size)
        .spawn(|| {
            let mut runner = TestRunner::deterministic();
            let tree = deeply_nested().new_tree(&mut runner).unwrap();
            std::mem::size_of_val(&tree)
        })
        .unwrap()
        .join()
        .unwrap()
}

#[test]
#[cfg_attr(not(feature = "boxed-runner"), ignore)]
fn deeply_nested_tree_fits_in_2mib_stack() {
    new_tree_on_stack(2 << 20);
}

// The value tree, which occupies the stack, is reduced by 93% with the
// `boxed-runner` feature, from about 179 KB to 13 KB.
#[test]
#[ignore]
fn deeply_nested_tree_size() {
    println!("tree size: {} B", new_tree_on_stack(64 << 20));
}
