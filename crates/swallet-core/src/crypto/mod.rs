pub mod encryption;
pub mod eth_keys;
pub mod mnemonic;
pub mod sol_keys;

/// 安全清零敏感数据，替代 zeroize crate
pub trait SecureClear {
    fn clear_sensitive(&mut self);
}

impl SecureClear for [u8] {
    fn clear_sensitive(&mut self) {
        // volatile write 防止编译器优化掉清零操作
        for byte in self.iter_mut() {
            unsafe { std::ptr::write_volatile(byte, 0) };
        }
        std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);
    }
}

impl SecureClear for Vec<u8> {
    fn clear_sensitive(&mut self) {
        self.as_mut_slice().clear_sensitive();
        self.clear();
    }
}

impl SecureClear for String {
    fn clear_sensitive(&mut self) {
        // SAFETY: 将所有字节置零，然后 clear 使长度归零
        unsafe { self.as_bytes_mut() }.clear_sensitive();
        self.clear();
    }
}
