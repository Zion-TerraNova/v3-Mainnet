// Canonical PoW kernels for V3 mainnet.
// Three algorithms only: Deeksha (full Ekam), Lite v1, Fire.
// Experimental variants (optimized/fire_optimized) live in DeekshaDebug/ sandbox only.
pub const COSMIC_HARMONY_DEEKSHA_KERNEL: &str = include_str!("kernels/cosmic_harmony_deeksha.cl");
pub const DEEKSHA_LITE_KERNEL: &str = include_str!("kernels/deeksha_lite.cl");
pub const DEEKSHA_LITE_FIRE_KERNEL: &str = include_str!("kernels/deeksha_lite_fire.cl");

pub const EKAM_DEEKSHA_KERNEL_NAME: &str = "ekam_deeksha_mine";
pub const EKAM_DEEKSHA_S4_KERNEL_NAME: &str = "ekam_deeksha_mine_s4";
pub const DEEKSHA_LITE_KERNEL_NAME: &str = "deeksha_lite_mine";
pub const DEEKSHA_LITE_FIRE_KERNEL_NAME: &str = "deeksha_lite_fire_mine";

pub fn get_deeksha_kernel_source() -> &'static str {
    COSMIC_HARMONY_DEEKSHA_KERNEL
}

pub fn get_deeksha_lite_kernel_source() -> &'static str {
    DEEKSHA_LITE_KERNEL
}

pub fn get_deeksha_lite_fire_kernel_source() -> &'static str {
    DEEKSHA_LITE_FIRE_KERNEL
}

pub fn has_ekam_deeksha_kernel() -> bool {
    COSMIC_HARMONY_DEEKSHA_KERNEL.contains(EKAM_DEEKSHA_KERNEL_NAME)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deeksha_kernel_is_present() {
        assert!(COSMIC_HARMONY_DEEKSHA_KERNEL.contains("__kernel"));
        assert!(has_ekam_deeksha_kernel());
    }
}
