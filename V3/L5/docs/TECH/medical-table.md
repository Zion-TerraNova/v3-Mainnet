# L5 Medical Table — Health Protocol Specification

> **Holistic health infrastructure for Terra Nova physical communities: from first aid to integrated wellness.**

---

## 1. Purpose

The Medical Table is not a hospital. It is a **community health protocol** that combines:
- **Emergency response** (trauma, acute illness)
- **Preventive care** (herbal medicine, nutrition, movement)
- **Diagnostic support** (bioresonance, PEMF, basic lab)
- **Wellness practice** (meditation, breathwork, bodywork)
- **Knowledge preservation** (bioregional herbalism, ethnobotany)

It operates at three levels of depth, scaling with community maturity.

---

## 2. Three Levels of Medical Table

### 2.1 Level 1: First Response (Phase 1 — Roots)

**Scope:** Stabilize, assess, decide: treat here or evacuate?

| Equipment | Purpose | Cost |
|-----------|---------|------|
| Professional first-aid kit (DIN 13169) | Trauma, bleeding, fracture, burns | EUR 150–300 |
| Automated External Defibrillator (AED) | Cardiac arrest | EUR 800–1,200 |
| Pulse oximeter + thermometer + BP cuff | Vital signs | EUR 100–200 |
| Emergency oxygen (portable concentrator) | Hypoxia, altitude sickness | EUR 300–600 |
| Emergency evacuation plan + 112/911 protocols | Legal + safety | N/A (document) |
| Tourniquet + chest seal + hemostatic gauze | Severe trauma | EUR 100 |
| SAM splint + traction splint | Fracture immobilization | EUR 80 |

**Personnel:** Minimum 1 Guardian with **Wilderness First Responder (WFR)** or equivalent certification. Renewal every 3 years.

**Protocols:**
- ABC (Airway, Breathing, Circulation) assessment
- RICE (Rest, Ice, Compression, Elevation) for soft tissue
- Evacuation decision tree: "Can we treat? / Should we transport? / How fast?"
- Emergency contact card for every guest and Guardian

### 2.2 Level 2: Community Herbal + Biophysical (Phase 2 — Community)

**Scope:** Treat common ailments, support chronic conditions, maintain wellness.

| Equipment | Purpose | Cost |
|-----------|---------|------|
| Herbal preparation station | Tinctures, teas, salves, syrups | EUR 300–500 |
| Drying rack + dehydrator | Preserve harvested herbs | EUR 150–300 |
| Bioresonance device (e.g., BICOM, Sensitiv Imago) | Frequency-based diagnostics | EUR 3,000–8,000 |
| PEMF mat (e.g., Bemer, Omnium1) | Pulsed electromagnetic field therapy | EUR 3,000–6,000 |
| Infrared sauna / sweat lodge | Detox, circulation, relaxation | EUR 500–2,000 |
| Basic lab (microscope, pH strips, glucose meter) | Simple diagnostics | EUR 200–400 |
| Medical records (paper + encrypted digital) | Track treatments, allergies, history | EUR 0–200 |

**Herbal pharmacy (bioregional):**
- **Portugal (Genesis Garden):** Lavender, rosemary, thyme, marigold, nettle, dandelion, mallow
- **La Palma (Dharma Temple):** Aloe vera, dragon tree sap (*Dracaena draco*), local *Bosea* species, Tenerife lavender
- **General:** Echinacea, ginger, turmeric, garlic, honey (antibacterial)

**Personnel:**
- 1 Guardian with **clinical herbalist** training (2+ years)
- 1 Guardian with **massage / bodywork** certification
- Volunteer network: visiting practitioners (acupuncturist, naturopath, etc.)

**Protocols:**
- Intake form: medical history, allergies, current medications, goals
- Treatment plan: herbal + lifestyle + follow-up schedule
- Contraindication check: herbs ↔ pharmaceuticals (use drug interaction database)
- Documentation: anonymized case records for knowledge commons

### 2.3 Level 3: Integrated Wellness Center (Phase 3 — Radiance)

**Scope:** Full-spectrum health combining allopathic, herbal, energetic, and psychological approaches.

| Equipment | Purpose | Cost |
|-----------|---------|------|
| Hiran-integrated diagnostic terminal | AI-assisted symptom analysis, differential diagnosis | 🔵 Future (Hiran v2.5+) |
| Hyperbaric chamber (portable) | Oxygen therapy, wound healing, altitude recovery | EUR 10,000–20,000 |
| Cold plunge + contrast therapy | Circulation, inflammation, mental resilience | EUR 2,000–5,000 |
| Floatation tank (open-source design) | Sensory deprivation, deep relaxation | EUR 5,000–10,000 |
| Full-spectrum light therapy | SAD, circadian rhythm, vitamin D synthesis | EUR 500–1,000 |
| Sound healing setup (crystal bowls, gongs) | Vibration therapy, meditation support | EUR 1,000–3,000 |

**Personnel:**
- Part-time MD or DO (holistic orientation)
- Full-time herbalist + nutritionist
- Bodywork practitioners (Rolfing, craniosacral, Thai massage)
- Mental health support (counselor, trauma-informed facilitator)

**Protocols:**
- Annual health assessment (biometric + subjective)
- Personalized wellness plan (herbal + movement + diet + mental)
- Chronic condition support (diabetes, hypertension, autoimmune — in collaboration with local healthcare system)
- End-of-life care (palliative support, hospice coordination)

---

## 3. The Medical Table as a Physical Space

### 3.1 Design Principles

| Principle | Implementation |
|-----------|---------------|
| **Clean but not sterile** | Natural materials, natural light, plants; not a hospital |
| **Quiet** | Acoustic insulation, away from communal noise |
| **Accessible** | Ground floor, wide doors, reachable at night |
| **Dual-purpose** | Treatment room + workshop space (herbal prep, education) |
| **Resilient** | Off-grid power, water, climate control |

### 3.2 Room Specifications

| Level | Area | Key Features |
|-------|------|--------------|
| Level 1 | 10–15 m² | First-aid station, wall-mounted equipment, emergency phone |
| Level 2 | 30–50 m² | Treatment room, herbal prep corner, bioresonance/PEMF area, bathroom |
| Level 3 | 80–150 m² | Multiple treatment rooms, open wellness space, cold plunge, sauna, education corner |

### 3.3 Off-Grid Requirements

| System | Requirement |
|--------|-------------|
| **Power** | 1–3 kWh/day (PEMF, bioresonance, lighting, refrigeration for herbs) |
| **Water** | 200–500 L/day (treatment, cleaning, herbal prep) |
| **Climate** | Heating/cooling for patient comfort (passive design + minimal active) |
| **Waste** | Herbal waste → compost. Medical waste (sharps) → municipal collection. |

---

## 4. Knowledge Commons

### 4.1 Bioregional Herbal Database

Every L5 community maintains a **local herbal database**:

```json
{
  "plant_id": "rosmarinus-officinalis-algarve",
  "common_names": {"en": "Rosemary", "pt": "Alecrim", "cs": "Rozmarýna"},
  "latin_name": "Rosmarinus officinalis",
  "local_variety": "Algarve coastal",
  "habitat": "Coastal scrub, full sun, well-drained soil",
  "harvest": {"part": "leaves and flowering tops", "season": "spring", "method": "cut 10cm from tip"},
  "preparations": [
    {"type": "tincture", "ratio": "1:5 in 60% alcohol", "dosage": "2–4 ml, 3x daily"},
    {"type": "infusion", "ratio": "1 tsp per cup", "dosage": "1 cup, 2–3x daily"},
    {"type": "essential oil", "usage": "inhalation, topical (diluted)"}
  ],
  "indications": ["memory support", "circulation", "digestion", "hair growth"],
  "contraindications": ["pregnancy (high doses)", "epilepsy", "hypertension (high doses)"],
  "interactions": ["blood thinners (caution)"],
  "preparation_date": "2026-05-21",
  "prepared_by": "herbalist-guardian-01",
  "verified_by": ["community-herbal-circle"]
}
```

### 4.2 Case Studies (Anonymized)

Communities contribute **anonymized treatment records** to the L5 knowledge commons:

```
Case ID: GG-2026-042
Condition: Persistent insomnia (6 months)
Approach: Herbal (passionflower + valerian tincture), breathwork, sleep hygiene
Outcome: Improved sleep latency after 2 weeks; resolved after 6 weeks
Follow-up: 3 months, no relapse
Contributed by: Genesis Garden (anonymized)
```

### 4.3 Training Pathways

| Level | Training | Duration | Provider |
|-------|----------|----------|----------|
| First Responder | Wilderness First Aid (WFA) | 16 hours | NOLS, WMA, local |
| Guardian Herbalist | Community herbalist course | 1–2 years | Self-directed + mentorship |
| Clinical Herbalist | 3–4 year program | 3–4 years | Herbalist schools (online + practice) |
| Bioresonance operator | Device-specific training | 40–80 hours | Device manufacturer |
| PEMF practitioner | Certification course | 20–40 hours | Device manufacturer |

---

## 5. Ethics and Boundaries

### 5.1 What the Medical Table Does NOT Do

| Service | Reason | Alternative |
|---------|--------|-------------|
| Emergency surgery | No sterile OR, no anesthesia | Stabilize + evacuate to hospital |
| Prescription pharmaceuticals | Illegal without MD license | Refer to local healthcare system |
| Cancer treatment | Beyond scope, legal risk | Integrative support alongside oncology |
| Childbirth (high-risk) | No neonatal ICU | Refer to hospital; low-risk home birth with midwife only |
| Mental health crisis (severe) | No psychiatric ward | Refer to emergency services |

### 5.2 Informed Consent

Every treatment requires **documented informed consent**:
- Explanation of approach (herbal, biophysical, allopathic)
- Known risks and side effects
- Alternative options
- Right to refuse or stop at any time
- Signature (or verbal consent with witness)

### 5.3 Scope of Practice

| Role | Can Do | Cannot Do |
|------|--------|-----------|
| First Responder | Stabilize, evacuate, basic first aid | Diagnose, prescribe, suture |
| Community Herbalist | Herbal preparations, lifestyle advice, basic intake | Diagnose disease, prescribe drugs, perform invasive procedures |
| Bioresonance Operator | Run scans, suggest imbalances, recommend lifestyle | Diagnose medical conditions, replace MD consultation |
| Part-time MD (if available) | Diagnose, prescribe (within local law), refer | Surgery (without OR), complex specialty care |

---

## 6. Integration with ZION Stack

### 6.1 L1 (Core)
- **Not directly connected.** Health data is too sensitive for public blockchain.
- Exception: Anonymized aggregate statistics (e.g., "100 herbal consultations in Q2 2027") can be attested on-chain for transparency.

### 6.2 L2 (DAO)
- **Treasury proposals:** Medical Table expansion funded via DAO proposals
- **Governance:** Health Guardian elected via DAO vote (reputation-weighted)

### 6.3 L4 (OASIS)
- **Quest integration:** "Complete 7-day herbal detox" → XP reward
- **Reputation:** Health knowledge contributors earn OASIS reputation
- **Knowledge Commons:** Herbal database accessible via OASIS interface

### 6.4 L5 Local Agent
- **Sensor integration:** PEMF usage, bioresonance scans, herbal inventory
- **Alerting:** Expiring herbal stock, equipment maintenance, certification renewals

---

## 7. Implementation Roadmap

| Phase | Timeline | Deliverable |
|-------|----------|-------------|
| Phase 0 | 2026 | WFR-certified Guardian at each community |
| Phase 1 | 2027 | Level 1 first-aid station operational |
| Phase 2 | 2028 | Level 2 herbal + biophysical center |
| Phase 3 | 2029 | Level 3 integrated wellness center (select communities) |
| Phase 4 | 2030+ | Hiran-integrated diagnostics, inter-node health data sharing |

---

## 8. Reference Links

| Resource | URL |
|----------|-----|
| Wilderness Medical Associates | https://www.wildmed.com/ |
| American Herbalists Guild | https://www.americanherbalistsguild.com/ |
| Meshtastic health alerts | `V3/L5/docs/TECH/mesh-network.md` |
| L5 Governance | `V3/L5/docs/GOVERNANCE/community-dao-framework.md` |
| ZION Core | `V3/L1/core/` |

---

> *"The Medical Table is not a replacement for modern medicine. It is a bridge — between the hospital and the garden, between the pill and the plant, between the diagnosis and the dialogue."*

*V3/L5/TECH · Medical Table Spec · 2026*
