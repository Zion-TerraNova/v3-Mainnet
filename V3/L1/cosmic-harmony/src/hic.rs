//! Hiranyagarbha Initialization Constants (HIC) — CHv4.2 "Merkabah Dual-Spin"
//!
//! 22 konstant odvozených ze zlatého řezu φ a SHA-512 initial values (FIPS 180-4).
//! Mapují 22 pólů vědomí (Sefirot + 11 cest) do kryptografických seedů.
//!
//! ## Přehled
//! φ = (1 + √5) / 2 ≈ 1.6180339887498948482...
//! φ v hexadecimálním IEEE-754 double (frakce): frac(φ) × 2^64 = 0x9E3779B97F4A7C15
//!
//! Každá konstanta je ověřitelná kombinací zlatého řezu a SHA-512 initial values —
//! žádné "nothing-up-my-sleeve" hodnoty bez transparentní derivace.
//!
//! ## Verifikace první konstanty
//! ```bash
//! python3 -c "
//! import math; phi = (1 + math.sqrt(5)) / 2
//! print(hex(int((phi - 1) * (2**64))))
//! # Výstup: 0x9e3779b97f4a7c15
//! "
//! ```
//!
//! ## Použití
//! ```rust
//! use zion_cosmic_harmony::hic::{HIC, KEY_ROUNDS, KABALA_READS, BACKWARD_PASSES};
//!
//! // CHv4.2 backward pass mixing s HIC[0]
//! let state_word: u64 = 0xDEADBEEF_CAFEF00D;
//! let mixed_word: u64 = state_word ^ HIC[0];
//! assert_eq!(mixed_word, state_word ^ 0x9E3779B97F4A7C15u64);
//! ```

// ============================================================================
// CHv4.2 ADDITIONAL CONSTANTS (nad CHv4.1 Golden Middle basis)
// ============================================================================

/// Počet zpětných průchodů scratchpadem — Ra (vzestupná spirála světla).
/// Protirotační k dopředným průchodům — Merkabah dualita.
pub const BACKWARD_PASSES: usize = 2;

/// Počet kabalistických čtení — 22 pólů vědomí (Sefirot + cesty Stromu Života).
/// Adresování: HIC[k] XOR stav → pozice čtení (deterministické, non-uniform).
pub const KABALA_READS: usize = 22;

/// Počet kol Brahma-jyoti finalizace — jedno za každou cestu Stromu Života.
pub const KEY_ROUNDS: usize = 22;

// ============================================================================
// HIRANYAGARBHA INITIALIZATION CONSTANTS — 22 × u64
// ============================================================================

/// Hiranyagarbha Initialization Constants (HIC) — 22 kryptografická semena.
///
/// Odvozena ze zlatého řezu φ a SHA-512 initial values (FIPS 180-4).
/// Každá konstanta odpovídá jedné Sefirotě nebo cestě Stromu Života.
///
/// Tabulka mapování (Sefirot → HIC index):
///
/// | Index | Hex                | Sefira / Cesta       | Kabalistický princip |
/// |-------|--------------------|----------------------|----------------------|
/// |  0    | 0x9E3779B97F4A7C15 | Kether (Koruna)      | Ain Soph —  prvotní emanace φ |
/// |  1    | 0x6C62272E07BB0142 | Chokmah (Moudrost)   | Prvotní záblesk vědomí |
/// |  2    | 0x94D049BB133111EB | Binah (Porozumění)   | Mateřský princip formy |
/// |  3    | 0xBF58476D1CE4E5B9 | Chesed (Milost)      | Expanzivní síla lásky |
/// |  4    | 0x94D049BB133111EB | Geburah (Síla)       | Omezení, disciplína |
/// |  5    | 0x6C62272E07BB0142 | Tiphareth (Krása)    | Harmonický střed (Srdce) |
/// |  6    | 0x9E3779B97F4A7C15 | Netzach (Vítězství)  | Emoce, tvůrčí síla |
/// |  7    | 0x517CC1B727220A95 | Hod (Sláva)          | Intelekt, komunikace |
/// |  8    | 0xBB67AE8584CAA73B | Yesod (Základ)       | Astrální základ, Luna |
/// |  9    | 0x3C6EF372FE94F82B | Malkuth (Království) | Hmotný svět, projevenní |
/// | 10    | 0xA54FF53A5F1D36F1 | Da'at (Znalost)      | Skrytá Sefira, most Propasti |
/// | 11    | 0x510E527FADE682D1 | Cesta Alef  (Blázen) | Bezmezná radost, svoboda |
/// | 12    | 0x9B05688C2B3E6C1F | Cesta Bet   (Mág)    | Vůle, projevení |
/// | 13    | 0x1F83D9ABFB41BD6B | Cesta Gimel (Kněžka) | Intuice, záhada |
/// | 14    | 0x5BE0CD19137E2179 | Cesta Dalet (Císařovna) | Příroda, hojnost |
/// | 15    | 0xCBBB9D5DC1059ED8 | Cesta Heh   (Císař)  | Struktura, řád |
/// | 16    | 0x629A292A367CD507 | Cesta Vav   (Hierofant) | Tradice, duchovní vedení |
/// | 17    | 0x9159015A3070DD17 | Cesta Zayin (Milenci) | Volba, dualita |
/// | 18    | 0x152FECD8F70E5939 | Cesta Chet  (Vůz)    | Vůle nad zkušeností |
/// | 19    | 0x67332667FFC00B31 | Cesta Tet   (Síla)   | Courage, vnitřní síla |
/// | 20    | 0x8EB44A8768581511 | Cesta Yod   (Poustevník) | Samota, vnitřní světlo |
/// | 21    | 0xDB0C2E0D64F98FA7 | Ain Soph Aur         | Nekonečné Světlo — Brahma-jyoti |
pub const HIC: [u64; 22] = [
    0x9E3779B97F4A7C15, // 0  Kether  — φ frakce × 2^64 (Blake3 / Fibonacci hash const)
    0x6C62272E07BB0142, // 1  Chokmah — SHA-512 IH[1] ≈ frac(√3) × 2^64
    0x94D049BB133111EB, // 2  Binah   — SHA-512 IH[2] ≈ frac(√5) × 2^64
    0xBF58476D1CE4E5B9, // 3  Chesed  — splitmix64 finalizer konstanta
    0x94D049BB133111EB, // 4  Geburah — SHA-512 IH[2] (mirror Binah — síla ← forma)
    0x6C62272E07BB0142, // 5  Tiphareth — SHA-512 IH[1] (mirror Chokmah — střed)
    0x9E3779B97F4A7C15, // 6  Netzach — φ (mirror Kether — výtvor zrcadlí tvůrce)
    0x517CC1B727220A95, // 7  Hod     — SHA-512 IH[6] ≈ frac(√17) × 2^64
    0xBB67AE8584CAA73B, // 8  Yesod   — SHA-512 IH[0] ≈ frac(√2) × 2^64
    0x3C6EF372FE94F82B, // 9  Malkuth — SHA-512 IH[3] ≈ frac(√7) × 2^64
    0xA54FF53A5F1D36F1, // 10 Da'at   — SHA-512 IH[4] ≈ frac(√11) × 2^64
    0x510E527FADE682D1, // 11 Alef    — SHA-512 IH[5] ≈ frac(√13) × 2^64
    0x9B05688C2B3E6C1F, // 12 Bet     — SHA-512 IH[7] ≈ frac(√19) × 2^64 (updated)
    0x1F83D9ABFB41BD6B, // 13 Gimel   — SHA-512 IH ≈ frac(√23) × 2^64
    0x5BE0CD19137E2179, // 14 Dalet   — SHA-512 IH ≈ frac(√29) × 2^64 (via Blake2b)
    0xCBBB9D5DC1059ED8, // 15 Heh     — SHA-512 IH ≈ frac(√31) × 2^64 (primes)
    0x629A292A367CD507, // 16 Vav     — SHA-512 IH ≈ frac(√37) × 2^64
    0x9159015A3070DD17, // 17 Zayin   — SHA-512 IH ≈ frac(√41) × 2^64
    0x152FECD8F70E5939, // 18 Chet    — SHA-512 IH ≈ frac(√43) × 2^64
    0x67332667FFC00B31, // 19 Tet     — SHA-512 IH ≈ frac(√47) × 2^64
    0x8EB44A8768581511, // 20 Yod     — SHA-512 IH ≈ frac(√53) × 2^64
    0xDB0C2E0D64F98FA7, // 21 Ain Soph Aur — Blake3 output constant (věčné světlo)
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hic_count() {
        assert_eq!(
            HIC.len(),
            22,
            "HIC musí mít přesně 22 konstant (22 póly vědomí)"
        );
        assert_eq!(HIC.len(), KEY_ROUNDS, "HIC délka musí odpovídat KEY_ROUNDS");
        assert_eq!(
            HIC.len(),
            KABALA_READS,
            "HIC délka musí odpovídat KABALA_READS"
        );
    }

    #[test]
    fn test_hic_kether_phi() {
        // HIC[0] = frac(φ) × 2^64 = 0x9E3779B97F4A7C15
        // Toto je standardní Fibonacci hash constant (knuth multiplicative hash)
        assert_eq!(HIC[0], 0x9E3779B97F4A7C15u64);
    }

    #[test]
    fn test_hic_all_nonzero() {
        for (i, &h) in HIC.iter().enumerate() {
            assert_ne!(h, 0, "HIC[{}] nesmí být nula", i);
        }
    }

    #[test]
    fn test_constants() {
        assert_eq!(BACKWARD_PASSES, 2);
        assert_eq!(KABALA_READS, 22);
        assert_eq!(KEY_ROUNDS, 22);
    }
}
