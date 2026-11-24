mod tests {
    use hd_wasm::testing::emit::Ctl;
    use hd_wasm::testing::emit::control::Stack;

    #[test]
    fn indexed_targets_count_linear_work_at_width_1000_depth_20() {
        let ((), work) = hd_wasm::work::count_control(|| {
            let mut stack = Stack::default();
            for arm in 0..1000 {
                stack.push(Ctl::Arm(7, arm));
                for _ in 0..20 {
                    stack.push(Ctl::Plain);
                }
            }
            for arm in (0..1000).rev() {
                assert_eq!(stack.position(Ctl::Arm(7, arm)), Some(21 * arm as usize));
                for _ in 0..20 {
                    assert!(stack.pop() == Some(Ctl::Plain));
                }
                assert!(stack.pop() == Some(Ctl::Arm(7, arm)));
            }
            assert_eq!(stack.len(), 0);
        });
        assert_eq!(work, 43_000);
    }

    #[test]
    fn popping_restores_shadowed_targets() {
        let mut stack = Stack::default();
        assert!(stack.pop().is_none());
        assert_eq!(stack.position(Ctl::Exit), None);
        assert_eq!(stack.position(Ctl::Plain), None);
        stack.push(Ctl::Plain);
        stack.push(Ctl::Plain);
        assert_eq!(stack.position(Ctl::Plain), Some(1));
        assert!(stack.pop() == Some(Ctl::Plain));
        assert_eq!(stack.position(Ctl::Plain), Some(0));
        assert!(stack.pop() == Some(Ctl::Plain));
        assert_eq!(stack.position(Ctl::Plain), None);
        stack.push(Ctl::Brk(2));
        stack.push(Ctl::Plain);
        stack.push(Ctl::Brk(2));
        assert_eq!(stack.position(Ctl::Brk(2)), Some(2));
        assert!(stack.pop() == Some(Ctl::Brk(2)));
        assert_eq!(stack.position(Ctl::Brk(2)), Some(0));
        assert!(stack.pop() == Some(Ctl::Plain));
        assert!(stack.pop() == Some(Ctl::Brk(2)));
        assert_eq!(stack.position(Ctl::Brk(2)), None);
    }
}
