//! NCL (Neural Compute Layer) Integration for CH v3
//!
//! Provides the 5th revenue stream: AI Compute Bonus
//!
//! Architecture (50/25/25 model):
//! - 50% compute → ZION mining (Keccak→SHA3→Matrix→Fusion)
//!   └── BONUS: Keccak & SHA3 intermediates submitted FREE to ETC/Nexus
//! - 25% compute → Multi-Algo profit-switch (ERG/RVN/KAS/ALPH)
//! - 25% compute → NCL AI inference tasks
//!
//! Revenue streams: 5 total, but only 3 cost compute!
//! - Stream 1: ZION (50% compute)
//! - Stream 2: Keccak/ETC (FREE byproduct of stream 1)
//! - Stream 3: SHA3/Nexus (FREE byproduct of stream 1)
//! - Stream 4: Multi-Algo (25% compute)
//! - Stream 5: NCL AI (25% compute)
//!
//! Integrates with NPU engines (CoreML, TensorRT, ONNX)
//!
//! ## Consciousness Levels — DISABLED for Mainnet (L1)
//!
//! Consciousness-based reward multipliers are disabled for the mainnet launch.
//! All levels return multiplier 1.0×. The enum and structures are preserved
//! for potential re-activation in Layer 3 (L3) post-mainnet.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

/// Consciousness levels — DISABLED for Mainnet (L1).
/// All multipliers are 1.0×. Reserved for future L3 activation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConsciousnessLevel {
    Physical,  // 1.0x
    Emotional, // 1.0x (disabled, was 1.05x — reserved for L3)
    Mental,    // 1.0x (disabled, was 1.1x  — reserved for L3)
    Spiritual, // 1.0x (disabled, was 1.25x — reserved for L3)
    Cosmic,    // 1.0x (disabled, was 1.5x  — reserved for L3)
    OnTheStar, // 1.0x (disabled, was 2.0x  — reserved for L3)
}

impl ConsciousnessLevel {
    /// Returns the reward multiplier for this consciousness level.
    ///
    /// **Mainnet (L1):** All levels return 1.0× (disabled).
    /// Differential multipliers are reserved for L3 post-mainnet.
    pub fn multiplier(&self) -> f64 {
        // Consciousness multipliers DISABLED for mainnet L1.
        // All levels return 1.0× — no unfair advantage.
        // Original values preserved in comments for future L3 activation:
        //   Physical=1.0, Emotional=1.05, Mental=1.1,
        //   Spiritual=1.25, Cosmic=1.5, OnTheStar=2.0
        1.0
    }

    pub fn from_level(level: u8) -> Self {
        match level {
            1 => Self::Physical,
            2 => Self::Emotional,
            3 => Self::Mental,
            4 => Self::Spiritual,
            5 => Self::Cosmic,
            6 => Self::OnTheStar,
            _ => Self::Physical,
        }
    }

    pub fn level(&self) -> u8 {
        match self {
            Self::Physical => 1,
            Self::Emotional => 2,
            Self::Mental => 3,
            Self::Spiritual => 4,
            Self::Cosmic => 5,
            Self::OnTheStar => 6,
        }
    }
}

/// Types of AI tasks for NCL
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AITaskType {
    Embeddings,
    LlmInference,
    ImageClassification,
    ImageGeneration,
    SpeechToText,
    CodeAnalysis,
    ModelTraining,
}

impl AITaskType {
    /// Base reward in ZION for each task type
    pub fn base_reward(&self) -> f64 {
        match self {
            Self::Embeddings => 0.001,
            Self::LlmInference => 0.01,
            Self::ImageClassification => 0.002,
            Self::ImageGeneration => 0.02,
            Self::SpeechToText => 0.005,
            Self::CodeAnalysis => 0.003,
            Self::ModelTraining => 0.1,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Embeddings => "embeddings",
            Self::LlmInference => "llm_inference",
            Self::ImageClassification => "image_classification",
            Self::ImageGeneration => "image_generation",
            Self::SpeechToText => "speech_to_text",
            Self::CodeAnalysis => "code_analysis",
            Self::ModelTraining => "model_training",
        }
    }
}

/// NPU Runtime detection
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NPURuntime {
    CoreML,   // Apple Silicon
    TensorRT, // NVIDIA
    OpenVINO, // Intel
    ONNX,     // Generic
}

impl NPURuntime {
    /// Detect best runtime for current platform
    pub fn detect() -> Self {
        #[cfg(target_os = "macos")]
        {
            #[cfg(target_arch = "aarch64")]
            return NPURuntime::CoreML;
            #[cfg(not(target_arch = "aarch64"))]
            return NPURuntime::ONNX;
        }

        #[cfg(not(target_os = "macos"))]
        {
            // Check for NVIDIA GPU (would need actual GPU detection)
            // For now default to ONNX
            NPURuntime::ONNX
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::CoreML => "coreml",
            Self::TensorRT => "tensorrt",
            Self::OpenVINO => "openvino",
            Self::ONNX => "onnx",
        }
    }
}

/// NCL Scheduler for time allocation
///
/// Compute split: 75% mining (50% ZION + 25% multi-algo), 25% NCL AI
/// Keccak & SHA3 intermediate hashes are FREE byproducts of ZION mining.
pub struct NCLScheduler {
    mining_allocation: f64, // 0.75 = 75% mining (50% ZION + 25% multi-algo)
    #[allow(dead_code)]
    min_mining: f64,
    #[allow(dead_code)]
    max_mining: f64,
    mining_time_ms: AtomicU64,
    npu_time_ms: AtomicU64,
    mining_priority: std::sync::atomic::AtomicBool,
}

impl NCLScheduler {
    pub fn new(mining_allocation: f64) -> Self {
        Self {
            mining_allocation: mining_allocation.clamp(0.5, 0.9),
            min_mining: 0.5,
            max_mining: 0.9,
            mining_time_ms: AtomicU64::new(0),
            npu_time_ms: AtomicU64::new(0),
            mining_priority: std::sync::atomic::AtomicBool::new(false),
        }
    }

    pub fn npu_allocation(&self) -> f64 {
        1.0 - self.mining_allocation
    }

    pub fn should_do_npu_work(&self) -> bool {
        if self.mining_priority.load(Ordering::Relaxed) {
            return false;
        }

        let mining = self.mining_time_ms.load(Ordering::Relaxed);
        let npu = self.npu_time_ms.load(Ordering::Relaxed);
        let total = mining + npu;

        if total == 0 {
            return true;
        }

        let ratio = mining as f64 / total as f64;
        ratio > self.mining_allocation
    }

    pub fn record_mining_time(&self, ms: u64) {
        self.mining_time_ms.fetch_add(ms, Ordering::Relaxed);
    }

    pub fn record_npu_time(&self, ms: u64) {
        self.npu_time_ms.fetch_add(ms, Ordering::Relaxed);
    }

    pub fn set_mining_priority(&self, priority: bool) {
        self.mining_priority.store(priority, Ordering::Relaxed);
    }

    pub fn reset(&self) {
        self.mining_time_ms.store(0, Ordering::Relaxed);
        self.npu_time_ms.store(0, Ordering::Relaxed);
    }

    pub fn stats(&self) -> NCLSchedulerStats {
        let mining = self.mining_time_ms.load(Ordering::Relaxed);
        let npu = self.npu_time_ms.load(Ordering::Relaxed);
        let total = mining + npu;

        NCLSchedulerStats {
            mining_allocation: self.mining_allocation,
            npu_allocation: self.npu_allocation(),
            actual_mining_ratio: if total > 0 {
                mining as f64 / total as f64
            } else {
                0.0
            },
            actual_npu_ratio: if total > 0 {
                npu as f64 / total as f64
            } else {
                0.0
            },
            mining_time_ms: mining,
            npu_time_ms: npu,
            mining_priority: self.mining_priority.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Clone)]
pub struct NCLSchedulerStats {
    pub mining_allocation: f64,
    pub npu_allocation: f64,
    pub actual_mining_ratio: f64,
    pub actual_npu_ratio: f64,
    pub mining_time_ms: u64,
    pub npu_time_ms: u64,
    pub mining_priority: bool,
}

/// NCL Bonus Calculator
pub struct NCLBonusCalculator {
    consciousness: ConsciousnessLevel,
    total_tasks: u64,
    successful_tasks: u64,
    total_latency_ms: u64,
}

impl NCLBonusCalculator {
    pub fn new(consciousness: ConsciousnessLevel) -> Self {
        Self {
            consciousness,
            total_tasks: 0,
            successful_tasks: 0,
            total_latency_ms: 0,
        }
    }

    pub fn calculate_reward(
        &mut self,
        task_type: AITaskType,
        execution_time_ms: u64,
        success: bool,
    ) -> f64 {
        let base_reward = task_type.base_reward();

        // Apply consciousness multiplier
        let mut reward = base_reward * self.consciousness.multiplier();

        // Update stats
        self.total_tasks += 1;
        if success {
            self.successful_tasks += 1;
            self.total_latency_ms += execution_time_ms;
        } else {
            // Reduced reward for failures
            reward *= 0.1;
        }

        // Efficiency bonus (up to 20% extra)
        let efficiency = self.efficiency();
        reward *= 1.0 + efficiency * 0.2;

        reward
    }

    pub fn efficiency(&self) -> f64 {
        if self.total_tasks == 0 {
            return 0.5;
        }

        // Success rate (50% weight)
        let success_rate = self.successful_tasks as f64 / self.total_tasks as f64;

        // Latency score (50% weight)
        let latency_score = if self.successful_tasks > 0 {
            let avg_latency = self.total_latency_ms as f64 / self.successful_tasks as f64;
            // Target: <100ms = 1.0, >1000ms = 0.0
            (1.0 - (avg_latency - 100.0) / 900.0).clamp(0.0, 1.0)
        } else {
            0.5
        };

        success_rate * 0.5 + latency_score * 0.5
    }

    pub fn stats(&self) -> NCLBonusStats {
        NCLBonusStats {
            consciousness: self.consciousness,
            total_tasks: self.total_tasks,
            successful_tasks: self.successful_tasks,
            success_rate: if self.total_tasks > 0 {
                self.successful_tasks as f64 / self.total_tasks as f64
            } else {
                0.0
            },
            efficiency: self.efficiency(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct NCLBonusStats {
    pub consciousness: ConsciousnessLevel,
    pub total_tasks: u64,
    pub successful_tasks: u64,
    pub success_rate: f64,
    pub efficiency: f64,
}

/// Complete NCL Integration for CH v3
pub struct NCLIntegration {
    pub miner_address: String,
    pub consciousness: ConsciousnessLevel,
    pub runtime: NPURuntime,
    pub scheduler: NCLScheduler,
    pub calculator: NCLBonusCalculator,

    // Stats
    pub tasks_completed: u64,
    pub tasks_failed: u64,
    pub total_earnings: f64,
    pub earnings_by_type: HashMap<AITaskType, f64>,
}

impl NCLIntegration {
    pub fn new(miner_address: String, consciousness_level: u8, mining_allocation: f64) -> Self {
        let consciousness = ConsciousnessLevel::from_level(consciousness_level);

        Self {
            miner_address,
            consciousness,
            runtime: NPURuntime::detect(),
            scheduler: NCLScheduler::new(mining_allocation),
            calculator: NCLBonusCalculator::new(consciousness),
            tasks_completed: 0,
            tasks_failed: 0,
            total_earnings: 0.0,
            earnings_by_type: HashMap::new(),
        }
    }

    /// Process an AI task and calculate reward
    pub fn process_task(
        &mut self,
        task_type: AITaskType,
        execution_time_ms: u64,
        success: bool,
    ) -> f64 {
        let reward = self
            .calculator
            .calculate_reward(task_type, execution_time_ms, success);

        if success {
            self.tasks_completed += 1;
        } else {
            self.tasks_failed += 1;
        }

        self.total_earnings += reward;
        *self.earnings_by_type.entry(task_type).or_insert(0.0) += reward;
        self.scheduler.record_npu_time(execution_time_ms);

        reward
    }

    /// Get the 5th revenue stream info
    pub fn revenue_stream(&self) -> NCLRevenueStream {
        NCLRevenueStream {
            stream_number: 5,
            name: "NCL AI Bonus".to_string(),
            source: "AI inference tasks".to_string(),
            allocation_percent: self.scheduler.npu_allocation() * 100.0,
            total_earnings: self.total_earnings,
            consciousness_multiplier: self.consciousness.multiplier(),
            efficiency: self.calculator.efficiency(),
        }
    }

    pub fn status(&self) -> NCLStatus {
        NCLStatus {
            miner_address: self.miner_address.clone(),
            consciousness: self.consciousness,
            runtime: self.runtime,
            scheduler: self.scheduler.stats(),
            bonus: self.calculator.stats(),
            tasks_completed: self.tasks_completed,
            tasks_failed: self.tasks_failed,
            total_earnings: self.total_earnings,
        }
    }
}

#[derive(Debug, Clone)]
pub struct NCLRevenueStream {
    pub stream_number: u8,
    pub name: String,
    pub source: String,
    pub allocation_percent: f64,
    pub total_earnings: f64,
    pub consciousness_multiplier: f64,
    pub efficiency: f64,
}

#[derive(Debug)]
pub struct NCLStatus {
    pub miner_address: String,
    pub consciousness: ConsciousnessLevel,
    pub runtime: NPURuntime,
    pub scheduler: NCLSchedulerStats,
    pub bonus: NCLBonusStats,
    pub tasks_completed: u64,
    pub tasks_failed: u64,
    pub total_earnings: f64,
}

// =============================================================================
// CH v3 Complete Revenue Model
// =============================================================================

/// Complete CH v3 with all 5 revenue streams (50/25/25 model)
///
/// Compute allocation: 50% ZION + 25% Multi-Algo + 25% NCL = 100%
/// Revenue streams: 5 (Keccak & SHA3 are FREE byproducts of ZION mining)
pub struct CH3RevenueModel {
    // Stream 1: ZION mining (50% compute → Keccak→SHA3→Matrix→Fusion)
    pub zion_earnings: f64,
    // Stream 2: ETC/NiceHash (FREE byproduct — Keccak intermediate from ZION pipeline)
    pub etc_earnings: f64,
    // Stream 3: Nexus/0xBTC (FREE byproduct — SHA3 intermediate from ZION pipeline)
    pub nxs_earnings: f64,
    // Stream 4: Multi-Algo profit-switch (25% compute → ERG/RVN/KAS/ALPH)
    pub dynamic_earnings: f64,
    // Stream 5: NCL AI inference (25% compute)
    pub ncl: NCLIntegration,
}

impl CH3RevenueModel {
    pub fn new(miner_address: &str, consciousness_level: u8) -> Self {
        Self {
            zion_earnings: 0.0,
            etc_earnings: 0.0,
            nxs_earnings: 0.0,
            dynamic_earnings: 0.0,
            ncl: NCLIntegration::new(
                miner_address.to_string(),
                consciousness_level,
                0.75, // 75% mining (50% ZION + 25% multi-algo), 25% NCL AI
            ),
        }
    }

    /// Total earnings across all 5 streams
    pub fn total_earnings(&self) -> f64 {
        self.zion_earnings
            + self.etc_earnings
            + self.nxs_earnings
            + self.dynamic_earnings
            + self.ncl.total_earnings
    }

    /// Revenue breakdown by stream
    pub fn revenue_breakdown(&self) -> Vec<(String, f64, f64)> {
        let total = self.total_earnings().max(0.0001); // Avoid div by zero

        vec![
            (
                "ZION (50% compute)".to_string(),
                self.zion_earnings,
                self.zion_earnings / total * 100.0,
            ),
            (
                "ETC/Keccak (FREE)".to_string(),
                self.etc_earnings,
                self.etc_earnings / total * 100.0,
            ),
            (
                "NXS/SHA3 (FREE)".to_string(),
                self.nxs_earnings,
                self.nxs_earnings / total * 100.0,
            ),
            (
                "Multi-Algo (25%)".to_string(),
                self.dynamic_earnings,
                self.dynamic_earnings / total * 100.0,
            ),
            (
                "NCL AI (25%)".to_string(),
                self.ncl.total_earnings,
                self.ncl.total_earnings / total * 100.0,
            ),
        ]
    }

    pub fn display(&self) {
        println!("\n╔════════════════════════════════════════════════════════════╗");
        println!("║     CH v3 REVENUE MODEL - 50/25/25 + 2 FREE BONUS         ║");
        println!("╠════════════════════════════════════════════════════════════╣");

        for (name, earnings, percent) in self.revenue_breakdown() {
            println!(
                "║  {:<25} {:>10.4} ZION  ({:>5.1}%)  ║",
                name, earnings, percent
            );
        }

        println!("╠════════════════════════════════════════════════════════════╣");
        println!(
            "║  TOTAL                      {:>10.4} ZION  (100%)    ║",
            self.total_earnings()
        );
        println!("╚════════════════════════════════════════════════════════════╝");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_consciousness_multiplier_disabled_for_mainnet() {
        // All consciousness levels return 1.0× on mainnet L1
        assert_eq!(ConsciousnessLevel::Physical.multiplier(), 1.0);
        assert_eq!(ConsciousnessLevel::Emotional.multiplier(), 1.0);
        assert_eq!(ConsciousnessLevel::Mental.multiplier(), 1.0);
        assert_eq!(ConsciousnessLevel::Spiritual.multiplier(), 1.0);
        assert_eq!(ConsciousnessLevel::Cosmic.multiplier(), 1.0);
        assert_eq!(ConsciousnessLevel::OnTheStar.multiplier(), 1.0);
    }

    #[test]
    fn test_ncl_scheduler() {
        let scheduler = NCLScheduler::new(0.75);
        assert!((scheduler.npu_allocation() - 0.25).abs() < 0.001); // 25% NCL

        // Initially should do NPU work
        assert!(scheduler.should_do_npu_work());

        // After lots of mining, should do NPU work
        scheduler.record_mining_time(1000);
        assert!(scheduler.should_do_npu_work());

        // Mining priority blocks NPU
        scheduler.set_mining_priority(true);
        assert!(!scheduler.should_do_npu_work());
    }

    #[test]
    fn test_bonus_calculator() {
        let mut calc = NCLBonusCalculator::new(ConsciousnessLevel::Cosmic);

        // All levels = 1.0x on mainnet L1 (consciousness disabled)
        let reward = calc.calculate_reward(AITaskType::Embeddings, 50, true);
        assert!(reward > 0.001); // Base * 1.0 + efficiency

        let reward2 = calc.calculate_reward(AITaskType::LlmInference, 100, true);
        assert!(reward2 > reward); // LLM has higher base reward
    }

    #[test]
    fn test_revenue_model() {
        let mut model = CH3RevenueModel::new("ZION_TEST", 5);

        // Add some earnings
        model.zion_earnings = 100.0;
        model.etc_earnings = 40.0;
        model.nxs_earnings = 10.0;
        model.dynamic_earnings = 40.0;
        model.ncl.process_task(AITaskType::Embeddings, 50, true);
        model.ncl.process_task(AITaskType::LlmInference, 100, true);

        let total = model.total_earnings();
        assert!(total > 190.0);

        let breakdown = model.revenue_breakdown();
        assert_eq!(breakdown.len(), 5);
    }
}
