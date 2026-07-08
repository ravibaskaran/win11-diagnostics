# Grants & Open Source Credits Strategy — sidebar

> **Document type:** Decision-grade investigation of OSS grants/credits/credits-equivalent programs applicable to `sidebar`, a greenfield Windows 11 desktop telemetry utility written in Rust, by a solo developer in India.
> **Author:** Research agent (ZCode).
> **Retrieval date for all cited facts:** 2026-07-07.
> **Scope:** All facts verified against primary 2026 sources via web search; no training-data reliance for program availability, pricing, or eligibility.

---

## 1. Executive Summary

**The $120/yr code-signing pain point is solvable for $0** — but not in the way most developers assume. There are two viable zero-cash paths and one near-zero one-time-cost path:

1. **SignPath Foundation (free, for OSS)** — issues an OV-level code-signing certificate to *SignPath Foundation itself* on your project's behalf and signs your binaries via a trusted GitHub Actions build. No recurring fee, no personal identity verification. The catch: the cert is in *their* name ("SignPath Foundation"), so it does **not** grant instant SmartScreen reputation — downloads will still warn initially while reputation accrues per-file-hash. This is the recommended path.
2. **Microsoft Store individual developer account (now free since Sept 2025)** — register at no cost, submit an MSIX package, and the binary ships *Microsoft-signed* via the Store, which **does** clear SmartScreen for Store-distributed installs. This is the recommended complementary path; it does not require a separate code-signing purchase.
3. **Azure Trusted Signing (paid, ~$120/yr)** — Microsoft's cloud signing, $9.99/month Basic. Useful if SignPath is rejected or if the user wants the cert in their own name, but **not free** and **not a grant**. Listed for completeness.

**Top 5 program recommendations** (ranked by weighted fit score, paid services excluded):

| Rank | Program | Why | Cash cost | Effort |
|---|---|---|---|---|
| 1 | **Microsoft Store individual dev account** | Now free (Sept 2025); gets Microsoft-signed MSIX distribution that clears SmartScreen for Store installs | $0 | Low |
| 2 | **SignPath Foundation for OSS** | Directly solves the $120/yr code-signing pain point for free (signed EXE/MSI via GitHub Actions); rolling applications | $0 | Low–Medium |
| 3 | **winget-pkgs distribution** | Free distribution via the built-in Windows package manager; trivial manifest PR | $0 | Trivial |
| 4 | **GitHub Sponsors (India supported)** | Self-serve donation channel for ongoing coffee-money funding; no application review | $0 | Trivial to set up |
| 5 | **Open Source Collective (fiscal host)** | Apply later if a funder requires a fiscal sponsor; 10% fee | $0 | Medium |

Both #1 and #2 are needed: SignPath signs the standalone GitHub Releases / winget binary; the Store handles the MSIX distribution path that bypasses SmartScreen for Store installs.

**Code-signing TL;DR:** Sign all releases with SignPath Foundation's cert via GitHub Actions (free), distribute the MSIX build via the Microsoft Store (free, Microsoft-signed), publish the standalone EXE/MSI to GitHub Releases + winget (free). Total annual cash outlay for signing + distribution: **$0**. A ~₹1,000–1,500/yr domain is the only non-grant cost worth incurring.

**Programs that look attractive but don't fit:** Vercel OSS, Netlify OSS, DigitalOcean, Cloudflare Project Alexandria, AWS OSS credits — these are all **infrastructure credits** for *server-side* workloads. This project is a desktop app. They would only legitimately apply to a tiny docs website or release-hosting edge case; do not apply to these for the desktop app itself.

**Dead programs (verified 2026-07-07):** Mozilla MOSS (indefinite hiatus since 2020 restructuring; not accepting applications). **Open Collective Foundation (OCF)** dissolved in 2024 — the surviving OSS fiscal host is **Open Source Collective (OSC)**, which charges a flat 10%.

**Rust Foundation reality check:** The Rust Foundation's flagship 2026 program is the **Maintainers Fund**, launched June 2026 — but it targets **maintainers of Rust-itself and crates owned by the Rust project**, not ecosystem crate authors and not application developers. The older Community Grants Program / Fellowships historically funded contributors working on Rust-language priorities; a greenfield solo desktop app is unlikely to qualify. Treat the Rust Foundation as a *watch-this-space* item, not an active application target.

---

## 2. Project Need Profile

What `sidebar` actually needs credits/money for, in descending priority:

| # | Need | Why it matters | Real cost (2026) | Best covered by |
|---|---|---|---|---|
| 1 | **Code signing certificate** | Removes Windows SmartScreen "unrecognized app" warnings; required for serious desktop distribution | $120/yr (Azure Basic) or $200–400/yr (traditional OV CA) or $400–900/yr (EV) | **SignPath Foundation** (free for OSS) |
| 2 | **SmartScreen reputation** | Even signed builds warn until enough downloads accrue; Store distribution bypasses this for Store installs | Time, not money | **Microsoft Store** MSIX distribution |
| 3 | **CI/CD runners (Windows)** | Build the EXE/MSIX on every push + release | $0 for public repos | **GitHub Actions** (free for public repos) |
| 4 | **Release artifact hosting / CDN** | Distribute the installer binaries | $0 | **GitHub Releases** (free, unlimited for public) |
| 5 | **Auto-update feed host** | Serve update manifests / version JSON | $0 | **GitHub Releases** + **GitHub Pages** (free) |
| 6 | **Docs website hosting** | Project README site / user guide | $0 | **Cloudflare Pages / GitHub Pages / Netlify OSS** (free tiers) |
| 7 | **Domain name** | Optional branding, ~$10–15/yr | ~₹1,000–1,500/yr | Out of pocket (no grant needed) |
| 8 | **Funding for time/hardware** | Solo dev sustainability | Variable | **GitHub Sponsors** (donation channel) |

**Bottom line:** Items 1–7 are all solvable for **$0 cash** using existing programs. The only real money question is whether to pay for an EV cert (no — overkill) or accept the one-time domain cost.

---

## 3. The Code-Signing Problem (Solved)

This is the user's stated pain point and deserves the most depth. Below is the realistic 2026 landscape for getting a signed Windows binary as a solo OSS developer.

### 3.1 The two-tier problem every Windows dev hits

Code signing on Windows has **two independent gates**:

1. **The signature itself** — a cryptographic attestation that the binary hasn't been tampered with and comes from a known publisher. Any valid cert (OV or EV) from a trusted CA passes this.
2. **SmartScreen Application Reputation (AppRep)** — Microsoft's *separate* reputation system that decides whether to show the "Windows protected your PC" blue warning. **A valid signature alone does not clear this.** Reputation accrues per (a) file hash and (b) signing certificate thumbprint over real-world download volume and clean behavior ([Microsoft Learn — SmartScreen reputation](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/smartscreen-reputation)).

This is why "I bought a $120 cert and I still see warnings" is a common complaint. The cert gives SmartScreen a *stable identity* to track; it doesn't *pre-establish* trust.

### 3.2 Option A — SignPath Foundation for OSS (RECOMMENDED, $0)

**What it is:** SignPath.io (commercial product) + SignPath Foundation (separate non-profit) together provide free code signing to qualifying OSS projects. The Foundation's certificate is issued in *their* name, and SignPath.io signs your binaries from a verified trusted build.

**What you get:**
- Free OV-level code-signing certificate (issued to "SignPath Foundation") — enough to pass the *signature* gate.
- SignPath.io subscription at no cost — the signing service/platform.
- GitHub Actions integration via `SignPath/github-action-submit-signing-request` ([GitHub Action](https://github.com/SignPath/github-action-submit-signing-request)) — signs EXE, MSI, MSIX, and other Windows formats.
- Built-in AV scanning, signature validation, timestamping.

**What you do NOT get (be honest about this):**
- ❌ **No instant SmartScreen reputation.** Because the cert is in SignPath Foundation's name and shared across many OSS projects, your specific binary still has to accrue per-hash reputation through downloads. Users will see the blue warning for the first N downloads until reputation builds.
- ❌ **No EV certificate** — OV only. EV certs (which historically granted instant reputation) are $400–900/yr and require hardware tokens; SignPath does not provide these for free.
- ❌ **No personal certificate in your name.** The publisher string is "SignPath Foundation", not you. For a solo OSS project this is fine; for a future commercial pivot you'd need your own cert.
- ❌ **Manual approval per release.** Each signing request requires a human approval step from your team (an "Approver" role). This is by design — prevents compromised CI from shipping malware.

**Eligibility requirements** ([SignPath Foundation terms](https://signpath.org/terms.html)) — must satisfy ALL:
- ✅ OSI-approved open source license (MIT, Apache-2.0, MPL-2.0, etc.) **with no commercial dual-licensing** for any component.
- ✅ No proprietary code anywhere — including no closed-source code written by you or an affiliated party. (System libraries like the Windows runtime are OK.)
- ✅ Public source repository.
- ✅ Project is **actively maintained** and **already released** in the form to be signed. *(Translation: you need at least one tagged release on GitHub before applying. Don't apply greenfield.)*
- ✅ Functionality documented on the download page / README.
- ✅ **No malware, no PUP, no hacking/security-exploitation tools.** ⚠️ **Critical for `sidebar`**: telemetry utilities that read hardware sensors via low-level APIs are *not* hacking tools and *should* qualify — but the OHM.exe bundled dependency (which reads CPU temps, fan RPMs, S.M.A.R.T. data) is borderline-adjacent territory. The risk is low because OHM is itself OSS and widely used; frame the project description carefully as a "system monitor" not a "system scanner."
- ✅ Multi-factor authentication on GitHub and SignPath for all team members.
- ✅ Define a team structure with **Authors**, **Reviewers**, and **Approvers** (even solo — you play all three roles).
- ✅ Publish a **code signing policy** on the project's home page.
- ✅ All builds must come from GitHub Actions on **GitHub-hosted agents** (not self-hosted runners).

**Application process:**
1. Have a public GitHub repo with LICENSE + README + at least one tagged release.
2. Apply via the SignPath Foundation site ([signpath.org](https://signpath.org/)).
3. Install the [SignPath GitHub App](https://github.com/apps/signpath) on the repo.
4. SignPath reviews (rolling; no fixed cycle).
5. On approval: create a `.signpath/policies/<project>/<signing-policy>.yml` file in your repo, add `SIGNPATH_API_TOKEN` to GitHub Actions secrets.
6. Wire the GitHub Action into your release workflow.

**Timeline:** Rolling review; community reports suggest 1–4 weeks from application to first signed build.

**Maintenance burden:** Low. Per-release manual approval click in SignPath UI. Annual re-review is *not* periodic (initial verification only, per their terms).

**Real-world Rust precedents:** Confirmed viable for Rust Windows apps. See [r/rust discussion](https://www.reddit.com/r/rust/comments/1tcz2od/what_are_my_options_for_code_signing_on_a_budget/) and [Microsoft Learn code-signing options](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/code-signing-options) (which explicitly endorses SignPath Foundation).

### 3.3 Option B — Microsoft Store individual dev account (RECOMMENDED, $0 since Sept 2025)

**Major 2026 update:** Microsoft **waived the $19 USD individual developer registration fee** in September 2025. As of May 2026, **company accounts ($99) are also free**. India is included. ([Windows Developer Blog Sept 2025](https://blogs.windows.com/windowsdeveloper/2025/09/10/free-developer-registration-for-individual-developers-on-microsoft-store/), [Windows Developer Blog May 2026](https://blogs.windows.com/windowsdeveloper/2026/05/07/publish-to-microsoft-store-as-a-company-now-with-free-registration-and-faster-onboarding/))

**What you get:**
- A Partner Center account at $0.
- Ability to submit MSIX packages to the Microsoft Store.
- Binaries distributed *through the Store* are **signed by Microsoft** — this **does** clear SmartScreen for Store-installed copies (the Store install path is trusted).
- Optional: store listing (you can list publicly or keep it unlisted/direct-link only).

**What you do NOT get:**
- ❌ Does not sign your *standalone* EXE/MSI on GitHub Releases — only the MSIX that goes through the Store.
- ❌ Store onboarding review can take hours to days; updates also go through review.
- ❌ MSIX packaging has sandboxing restrictions that may conflict with a hardware-telemetry app (e.g., reading certain system info, bundling `OpenHardwareMonitor.exe` may need elevated capabilities / restricted capabilities / runFullTrust). Expect friction here.
- ❌ The new free flow uses **ID-based verification** (government ID + selfie) instead of a fee — ensure you have a valid Indian government ID (Aadhaar/PAN/passport).

**Cost:** $0 (registration). No annual renewal fee.

**Recommendation:** Apply for both SignPath *and* the Store account. Use SignPath to sign the standalone GitHub Releases / winget build; use the Store for users who prefer Store installs. Two distribution channels, both free.

### 3.4 Option C — Azure Trusted Signing (PAID, ~$120/yr, NOT a grant)

For completeness and honesty. Microsoft's cloud signing service (formerly "Trusted Signing", now being marketed as "Azure Artifact Signing"). [Azure pricing](https://azure.microsoft.com/en-us/pricing/details/artifact-signing/).

| Tier | Monthly | Annualized | Signatures/mo | Cert profiles |
|---|---|---|---|---|
| **Basic** | $9.99 | ~$120 | 5,000 | 1 of each type |
| **Premium** | $99.99 | ~$1,200 | 100,000 | 10 of each type |
| Overage | $0.005/signature | — | beyond quota | — |

**When to use this instead of SignPath:**
- SignPath rejected your application.
- You want the cert in *your* (or your legal entity's) name rather than SignPath Foundation's.
- You need >5,000 signatures/month (unlikely for a desktop app — a release is one signature per binary).
- You want Microsoft-grade audit/compliance.

**Geographic availability:** India regions (Central India, South India, West India) are listed in the pricing page region selector, so an Indian individual can sign up. Individual developer sign-up opened in public preview (per Microsoft's Jan 2026 announcement). Still requires an Azure subscription with a payment method on file.

**Honest verdict:** For a solo OSS project, this is the *fallback* if SignPath doesn't work out. It's the same ~$120/yr the user is trying to avoid — but at least it's the cloud-native path that doesn't require a hardware token (unlike traditional EV certs).

### 3.5 Option D — Self-signed + reputation building ($0, painful)

Generate a self-signed code-signing cert, sign your binaries, distribute them. **This does not clear SmartScreen at all** — self-signed certs are not from a trusted CA, so Windows will treat the binary as *more* suspicious than an unsigned one in some configurations.

**Verdict:** Do not do this. It is worse than unsigned for end users.

### 3.6 The recommended zero-cash path (concrete recipe)

```
┌─────────────────────────────────────────────────────────────┐
│  sidebar code-signing + distribution (target: $0 cash)      │
└─────────────────────────────────────────────────────────────┘

  Rust source (GitHub, public, MIT or MPL-2.0)
        │
        ▼
  GitHub Actions (windows-latest, GitHub-hosted)
        │
        ├──► cargo build --release  →  sidebar.exe
        ├──► cargo-wix              →  sidebar.msi   (standalone installer)
        └──► MSIX packaging         →  sidebar.msix  (Store package)
        │
        ▼
  ┌─────────────────────────────────────────────┐
  │  SignPath GitHub Action                     │
  │  (signs sidebar.exe + sidebar.msi with      │
  │   SignPath Foundation OV cert, free)        │
  └─────────────────────────────────────────────┘
        │
        ├──► GitHub Releases (signed EXE + MSI)  ← FREE hosting
        ├──► winget manifest PR to microsoft/winget-pkgs  ← FREE
        └──► Microsoft Store submission (MSIX)   ← Microsoft-signed, free account
                                                   (Store install path bypasses SmartScreen)
```

**Expected user experience:**
- **Store install:** Zero warnings. Microsoft-signed. Clean.
- **winget install:** Uses the GitHub Releases signed binary. SmartScreen may warn on the very first downloads after a new release until per-hash reputation accrues (days to weeks depending on volume). After that: clean.
- **Direct download from GitHub Releases + run:** Same — initial warning, then clean.

**Honest caveat:** There is no free path to *zero* SmartScreen warnings on the direct-download build from day one. SignPath's OV cert + time is the realistic floor. If zero warnings on direct download from day one is a hard requirement, the only options are (a) an EV cert (~$400+/yr, not free anywhere) or (b) push everyone through the Store.

---

## 4. Program Survey

All programs investigated. "Fit note" is project-specific to `sidebar` (Windows desktop Rust utility, solo, India, greenfield).

### 4.1 Code signing & distribution (direct value to this project)

| Program | Provider | What they give | Eligibility (summary) | Process | Rolling? | Fit note |
|---|---|---|---|---|---|---|
| **SignPath Foundation for OSS** | SignPath Foundation (non-profit) + SignPath.io (commercial) | Free OV code signing + signing platform + GitHub Action integration | OSI license, public repo, released, no malware/PUP/hacking tools, MFA, GitHub-hosted CI | Apply at signpath.org | Rolling | ★★★★★ Directly solves the pain point |
| **Microsoft Store individual dev account** | Microsoft | Free Store distribution; Microsoft-signed MSIX | Government ID + selfie verification | Partner Center registration | Rolling (free since Sept 2025) | ★★★★★ Free since Sept 2025; complements SignPath |
| **winget-pkgs** | Microsoft (community) | Free package-manager distribution | Manifest YAML PR to microsoft/winget-pkgs | GitHub PR | Rolling | ★★★★☆ Free distribution; no signing required to submit |
| **Azure Trusted Signing** | Microsoft (paid) | Cloud code signing | Azure account + payment method | Self-serve | Rolling | ★★☆☆☆ Paid ~$120/yr — fallback only |

### 4.2 Rust ecosystem

| Program | Provider | What they give | Eligibility | Process | Rolling? | Fit note |
|---|---|---|---|---|---|---|
| **Rust Foundation Maintainers Fund** | Rust Foundation | Stable funding for maintainers | "Maintainers of Rust itself and any crate owned by the Rust project" — NOT ecosystem apps | Open call TBD (launched June 2026) | TBD | ★☆☆☆☆ Doesn't fit — targets Rust-language maintainers, not app developers |
| **Rust Foundation Community Grants Program / Fellowships** | Rust Foundation | Stipends (~$1,000/mo historically), travel, training | Historically: contributors working on Rust-priority work; competitive | Annual cycle (historically) | Cycle | ★☆☆☆☆ Greenfield app doesn't fit the "contributor to Rust" profile |
| **Rust Foundation Infrastructure grants** | Rust Foundation | Infra funding for critical Rust infra | Targeted at projects under Rust Foundation purview | Invitation/cycle | Cycle | ☆☆☆☆☆ Not applicable to a third-party app |

### 4.3 Fiscal sponsorship / donations

| Program | Provider | What they give | Eligibility | Process | Rolling? | Fit note |
|---|---|---|---|---|---|---|
| **GitHub Sponsors** | GitHub | Donation channel (receive funds) | Reside in supported region (India ✅), 18+, contribute to OSS | Self-serve profile setup | Rolling | ★★★★☆ India supported; easy; low friction; not a grant, just a channel |
| **Open Source Collective (OSC)** | OSC (501(c)(6) non-profit) | Fiscal sponsorship for OSS projects; receives grants/donations on your behalf | OSS project, OSI license | Apply via oscollective.org | Rolling | ★★★☆☆ Useful if you ever receive a grant that requires a fiscal sponsor; 10% fee |
| **Software Freedom Conservancy (SFC)** | SFC (501(c)(3)) | Fiscal sponsorship + legal/infra services | FOSS project, mission-aligned, existing track record | Email apply@sfconservancy.org with brief inquiry | Rolling | ★★☆☆☆ High bar — SFC is selective; greenfield project unlikely; useful later |
| **Open Collective (platform)** | Open Collective | Platform for fiscal hosting | Self-serve (find a host) | Self-serve | Rolling | ★★★☆☆ Platform, not a funder; use OSC as the host for OSS |

### 4.4 Cloud infrastructure credits (mostly poor fit for a desktop app)

| Program | Provider | What they give | Eligibility | Fit note |
|---|---|---|---|---|
| **Vercel Open Source Program** | Vercel | $3,600/yr in Vercel credits, quarterly cohorts | Actively maintained OSS, hosted/intended for Vercel, measurable impact | ★☆☆☆☆ Web-hosting credits. Only legitimate use: a small docs/landing site. Hobby tier is already free. Skip. |
| **Netlify Open Source Plan** | Netlify | Free hosting/bandwidth for OSS | OSI license, Code of Conduct, link back to Netlify | ★☆☆☆☆ Same as Vercel — docs site only. Free Hobby tier already covers this. |
| **DigitalOcean Open Source Credits** | DigitalOcean | DO credits (Droplets, storage, K8s) | Fully FOSS, OSI license, community impact, active | ★☆☆☆☆ VM/storage credits. Desktop app has no server-side workload. Skip. |
| **Cloudflare Project Alexandria** | Cloudflare | Recurring annual credits for Workers/Pages/R2 | Non-profit-aligned OSS | ★★☆☆☆ Could host a docs site or a tiny release-asset edge cache. Cloudflare's free tier is already very generous; project likely doesn't need more. Low priority. |
| **AWS Promotional Credits for OSS** | AWS | AWS credits | OSI license, not single-vendor-dominated, not VC-funded | ★☆☆☆☆ AWS credits = server-side. Desktop app has no AWS workload. Skip. |
| **GitLab for Open Source** | GitLab | Free Ultimate tier + 50,000 CI minutes/month | Every project in namespace has OSI license, public | ★★★☆☆ Legit free CI alternative to GitHub Actions. Useful only if you migrate off GitHub. Most Rust OSS stays on GitHub for network effects. Marginal value. |
| **Azure for Open Source / Microsoft programs** | Microsoft | Various; no direct desktop signing grant | N/A | ★☆☆☆☆ No equivalent of "free Azure signing for OSS" exists. Azure Trusted Signing is paid (see §3.4). |

### 4.5 Foundations & grant-making bodies

| Program | Provider | What they give | Eligibility | Fit note |
|---|---|---|---|---|
| **NLnet Foundation** | NLnet (NL) | €5,000–€50,000 grants for FOSS that benefits the open internet | FOSS, public-interest angle, applications via NLnet portal | ★★☆☆☆ Active (13th call: Apr 1 → Jun 1, 2026; rolling calls). A hardware telemetry sidebar is a stretch for "open internet" framing. Possible but not high-odds. |
| **Sovereign Tech Fund / Agency** | Germany (SPRIND) | Strategic investments in critical OSS (€24.6M+ to 60+ projects) | Critical digital infrastructure; primarily EU-relevant | ☆☆☆☆☆ Targets critical infrastructure (curl, ffmpeg, Scala, etc.). A greenfield solo desktop utility does not qualify. |
| **Mozilla MOSS** | Mozilla | Historically $10K–$100K grants | — | ❌ **DEAD.** Indefinite hiatus since 2020 restructuring. Not accepting applications. ([mozilla.org/en-US/moss](https://www.mozilla.org/en-US/moss/)) |
| **FLOSS/fund** | Zerodha (Nithin Kamath, India) | Up to $1M/yr total; individual grants to FOSS projects | Global, OSS, publicly accessible `funding.json` file, "critical, impactful, valuable" projects | ★★★☆☆ India-origin funder. Targets *existing, widely used, impactful* projects — a greenfield app likely won't qualify at launch, but worth watching once the project has traction. |
| **NumFOCUS** | NumFOCUS (US) | Fiscal sponsorship | Open source scientific computing | ☆☆☆☆☆ Scientific computing focus (NumPy, pandas, Jupyter). A telemetry sidebar is not scientific software. No fit. |
| **OpenSSF** | OpenSSF / Linux Foundation | Security-focused funding/tools | Security-critical OSS infrastructure | ☆☆☆☆☆ Targets supply-chain security of critical infrastructure. No fit for an end-user desktop app. |
| **.NET Foundation** | .NET Foundation | Sponsorship for .NET OSS | .NET ecosystem | ☆☆☆☆☆ Even though OHM is C#, the Rust *application* is not .NET. No fit. |
| **Linux Foundation scholarships/travel/minigrants** | Linux Foundation | Various | Varies | ☆☆☆☆☆ Mostly travel/education/grants for existing LF projects. No direct fit. |

### 4.6 India-specific

| Program | Provider | What they give | Fit note |
|---|---|---|---|
| **MeitY Startup Hub (MSH)** / SAMRIDH / Digital India Scale Up | Government of India | Startup funding/acceleration | ☆☆☆☆☆ **Startup** programs, not OSS grants. Require startup entity, traction, accelerators. Solo OSS project doesn't fit. |
| **ELEVATE 2026 (Startup Karnataka)** | Karnataka state | Grant-in-aid to startups | ☆☆☆☆☆ Same — startup-focused, not OSS. |
| **MeitY open source promotion (dedicated)** | — | — | ❌ No dedicated Indian government OSS-promotion grant program for individual developers was found in 2026 searches. Honest answer: this doesn't exist in a form usable by a solo OSS developer. |

---

## 5. Weighted Fit Analysis

Scoring rubric (each criterion 0–10, weighted):
- **Fit (40%)** — How well does the program's purpose match a Windows desktop Rust utility?
- **Eligibility likelihood (25%)** — Does a greenfield solo-developer Rust project from India realistically qualify?
- **Value to this project (25%)** — In dollar/in-kind terms relative to actual needs.
- **Application effort (10%)** — Rolling/self-serve = high score; competitive written application = lower.

Weighted score = (Fit × 0.40) + (Elig × 0.25) + (Value × 0.25) + (Effort × 0.10). Max = 10.0.

> Sorted by weighted score. Paid programs (Azure Trusted Signing) and dead programs (Mozilla MOSS) are listed but excluded from the "Top rec" tier.

| Rank | Program | Fit (40%) | Elig (25%) | Value (25%) | Effort (10%) | **Weighted** | Tier |
|---|---|---|---|---|---|---|---|
| 1 | **Microsoft Store individual dev (free)** | 9 | 9 | 9 | 9 | **9.00** | 🟢 Top rec |
| 2 | **SignPath Foundation for OSS** | 10 | 7 | 10 | 7 | **8.95** | 🟢 Top rec |
| — | Azure Trusted Signing *(paid fallback)* | 9 | 8 | 7 | 9 | **8.10** | 🟠 Paid only |
| 3 | **winget-pkgs distribution** | 8 | 10 | 6 | 9 | **7.85** | 🟢 Top rec |
| 4 | **GitHub Sponsors (donation channel)** | 7 | 10 | 5 | 10 | **7.55** | 🟢 Top rec |
| 5 | **Open Source Collective (fiscal host)** | 6 | 8 | 6 | 8 | **6.70** | 🟡 Apply later |
| 6 | **FLOSS/fund (Zerodha)** | 7 | 3 | 9 | 6 | **6.25** | 🟡 After traction |
| 7 | **GitLab for Open Source** | 6 | 9 | 4 | 8 | **6.05** | 🟡 Marginal |
| 8 | **NLnet Foundation** | 5 | 4 | 8 | 4 | **5.55** | 🟡 Stretch |
| 9 | **Cloudflare Project Alexandria** | 4 | 6 | 3 | 6 | **4.45** | 🟠 Docs site only |
| 10 | **Software Freedom Conservancy** | 5 | 2 | 6 | 3 | **4.35** | 🔴 High bar |
| 11 | **Netlify Open Source Plan** | 3 | 6 | 3 | 6 | **4.05** | 🟠 Docs site only |
| 12 | **Vercel Open Source Program** | 3 | 5 | 3 | 5 | **3.70** | 🟠 Docs site only |
| 13 | **DigitalOcean OSS Credits** | 2 | 6 | 2 | 6 | **3.50** | 🔴 No server workload |
| 14 | **AWS OSS Credits** | 2 | 6 | 2 | 5 | **3.45** | 🔴 No server workload |
| 15 | **Rust Foundation Fellowships** | 3 | 2 | 5 | 3 | **3.30** | 🔴 Doesn't fit |
| 16 | **Rust Foundation Maintainers Fund** | 2 | 1 | 5 | 2 | **2.45** | 🔴 Doesn't fit |
| 17 | **Sovereign Tech Fund** | 1 | 1 | 6 | 2 | **2.35** | 🔴 Doesn't fit |
| 18 | **NumFOCUS** | 1 | 1 | 3 | 3 | **1.85** | 🔴 Doesn't fit |
| — | Mozilla MOSS *(dead program)* | — | — | — | — | **N/A** | ❌ Dead |

**Top 5 (the realistic recommendations):** Microsoft Store, SignPath Foundation, winget-pkgs, GitHub Sponsors, Open Source Collective. Azure Trusted Signing ranks 3rd by raw score but is excluded from recommendations because it is a paid service (~$120/yr), not a grant.

---

## 6. Top Recommendations — Prescriptive Application Guide

### 6.1 SignPath Foundation for OSS

- **Direct application URL:** [signpath.org](https://signpath.org/) (apply via the Foundation site; integrate via [SignPath GitHub App](https://github.com/apps/signpath)).
- **Docs:** [docs.signpath.io](https://docs.signpath.io/) — start with [GitHub trusted build system docs](https://docs.signpath.io/trusted-build-systems/github).
- **Terms:** [signpath.org/terms.html](https://signpath.org/terms.html).

**Eligibility checklist (must satisfy ALL before applying):**
- [ ] Public GitHub repository.
- [ ] OSI-approved LICENSE file in repo root (MIT, Apache-2.0, or MPL-2.0 recommended for `sidebar`).
- [ ] At least one tagged GitHub Release with a built binary (don't apply greenfield).
- [ ] README documenting what the project does.
- [ ] No proprietary code; `OpenHardwareMonitor.exe` is itself MPL-2.0 OSS so this is fine — *but document its inclusion and license clearly*.
- [ ] No malware/PUP/hacking-tool functionality.
- [ ] MFA enabled on your GitHub account.
- [ ] Willingness to publish a code signing policy page in the repo/README.

**Required materials:**
- Repo URL.
- License URL.
- Description of project purpose.
- Confirmation that builds come from GitHub Actions on GitHub-hosted runners.

**Step-by-step process:**
1. Get the project to a "released" state: tag v0.1.0, push, build artifacts via GitHub Actions.
2. Install the [SignPath GitHub App](https://github.com/apps/signpath) on the repo.
3. Apply at [signpath.org](https://signpath.org/) with project details.
4. On approval, create `.signpath/policies/<project-slug>/<signing-policy-slug>.yml` in your repo (per the [GitHub integration docs](https://docs.signpath.io/trusted-build-systems/github)).
5. Add `SIGNPATH_API_TOKEN`, `SIGNPATH_ORGANIZATION_ID`, `SIGNPATH_PROJECT_SLUG`, `SIGNPATH_SIGNING_POLICY_SLUG` to GitHub Actions secrets.
6. Wire `SignPath/github-action-submit-signing-request@v2` into your release workflow:

```yaml
- name: Upload unsigned artifact
  id: upload-unsigned-artifact
  uses: actions/upload-artifact@v4
  with:
    path: target/release/sidebar.exe

- name: Submit signing request
  uses: signpath/github-action-submit-signing-request@v2
  with:
    api-token: '${{ secrets.SIGNPATH_API_TOKEN }}'
    organization-id: '<your-org-id>'
    project-slug: 'sidebar'
    signing-policy-slug: 'release-signing'
    github-artifact-id: '${{ steps.upload-unsigned-artifact.outputs.artifact-id }}'
    wait-for-completion: true
    output-artifact-directory: './signed'
```

**Realistic timeline:** 1–4 weeks from application to first signed build.

**What to write in the application (draft — adapt):**
> `sidebar` is an open-source Windows 11 desktop sidebar utility written in Rust that displays live hardware telemetry (CPU/GPU temperatures, clocks, utilization, RAM, drives, processes, network throughput, and monthly bandwidth tracking). It is licensed under [MIT / MPL-2.0] and distributes OpenHardwareMonitor.exe (MPL-2.0) as a bundled dependency for low-level sensor access. All releases are built reproducibly via GitHub Actions on GitHub-hosted Windows runners, published as GitHub Releases, and installable via winget and the Microsoft Store. We are applying for free OSS code signing to remove SmartScreen "unrecognized app" warnings for end users, who are typically enthusiasts and system administrators. The project is non-commercial, single-maintainer, and has no proprietary components.

**Common rejection reasons:**
- ❌ "Released" requirement not yet met (no tagged release).
- ❌ License incompatibility (no OSI license, or commercial dual-licensing).
- ❌ Hacking-tool classification (frame carefully — `sidebar` is a *system monitor*, not a *vulnerability scanner*).
- ❌ Self-hosted CI runners (must use GitHub-hosted).
- ❌ Bundled proprietary code.

**Renewal/maintenance burden:**
- Per-release manual approval click in SignPath UI.
- No periodic re-review (initial verification is one-time, per their terms).
- Must remain in compliance with the Code of Conduct; violations can result in cert revocation.

### 6.2 Microsoft Store individual developer account

- **Direct URL:** [developer.microsoft.com/en-us/microsoft-store/register](https://developer.microsoft.com/en-us/microsoft-store/register).
- **Free registration docs:** [learn.microsoft.com/en-us/windows/apps/publish/whats-new-individual-developer](https://learn.microsoft.com/en-us/windows/apps/publish/whats-new-individual-developer).
- **Pricing:** **$0** (since Sept 2025; previously $19 one-time). Company accounts also free since May 2026.

**Eligibility checklist:**
- [ ] Personal Microsoft account.
- [ ] Valid government-issued photo ID (Aadhaar / PAN / passport / driver's license — India accepted).
- [ ] Willingness to do selfie-based ID verification.
- [ ] A packaged MSIX for the app.

**Required materials:**
- Government ID.
- App package: MSIX (preferred). EXE/MSI installers are supported via MSIX conversion or the Store's MSIX-aware packaging, but MSIX-native is cleanest.
- Store listing assets (icons, screenshots, description) — can be minimal/unlisted.

**Step-by-step process:**
1. Sign in at Partner Center with a personal Microsoft account.
2. Select "Individual" account type.
3. Complete ID verification (government ID + selfie).
4. Reserve the app name "sidebar" (or chosen name).
5. Build the MSIX package (`cargo build --release` → MSIX via `msix-hero`, `MakeAppx`, or a WiX-based MSIX).
6. Submit the package + listing.
7. Microsoft review (typically hours to a few days).
8. Publish.

**Realistic timeline:** Account verification: 1–3 days. First app review: 1–7 days. Subsequent updates: hours to ~1 day.

**Common friction points (project-specific):**
- ⚠️ MSIX sandboxing vs. hardware telemetry. Reading CPU temps / S.M.A.R.T. data / network stats often requires `runFullTrust` capability or restricted capabilities. Expect to declare `rescap:runFullTrust` and justify it. Microsoft may push back; plan for one or two review iterations.
- ⚠️ Bundled `OpenHardwareMonitor.exe` running elevated may trigger review flags. Document why it's needed.
- ⚠️ App name reservation is first-come-first-served; check availability before committing to "sidebar" as the public name.

**What to write in your submission notes (draft):**
> `sidebar` is a desktop utility for Windows 11 that displays hardware telemetry (CPU, GPU, RAM, drives, network) in a screen-edge overlay. It requires full trust because it reads system sensor data via WMI, performance counters, and a bundled OpenHardwareMonitor.exe helper (open source, MPL-2.0). No network egress except optional auto-update checks against GitHub Releases. No telemetry collection from users.

**Renewal/maintenance burden:** None financial. Maintain Store listing; submit updates through Partner Center review.

### 6.3 GitHub Sponsors (donation channel)

- **Direct URL:** [github.com/sponsors](https://github.com/sponsors).
- **India supported:** Yes ([GitHub Blog — Sponsors launches in India](https://github.blog/news-insights/company-news/github-sponsors-launches-in-india/)).
- **Docs:** [docs.github.com/en/sponsors](https://docs.github.com/en/sponsors/getting-started-with-github-sponsors/about-github-sponsors).

**Eligibility checklist:**
- [ ] Reside in a supported region (India ✅).
- [ ] 18+ years old.
- [ ] Active GitHub profile.
- [ ] Two-factor authentication enabled.
- [ ] Bank account in a supported country (India ✅ — via Stripe / payout processor).
- [ ] Tax information (India PAN).

**Process:** Self-serve. Fill out the Sponsors profile, add bank/tax info, set up tier(s) ($1, $5, $10/mo are common for OSS). GitHub reviews the profile (usually 1–7 days), then it goes live.

**Value:** Low immediate, high optionality. Sets up the *plumbing* to receive money if the project gets traction. No downside to setting it up early.

**Renewal/maintenance burden:** None.

### 6.4 winget-pkgs distribution

- **Direct URL:** [github.com/microsoft/winget-pkgs](https://github.com/microsoft/winget-pkgs) — submit a manifest YAML via PR.
- **Docs:** [learn.microsoft.com/en-us/windows/package-manager/winget](https://learn.microsoft.com/en-us/windows/package-manager/winget/).

**Eligibility:** None — anyone can submit a manifest for any app (including their own).

**Process:** Fork `microsoft/winget-pkgs`, use `wingetcreate` to generate a manifest YAML for your GitHub Releases artifact, open a PR. Microsoft bot validates; merged within days.

**Value:** Free distribution to the built-in Windows package manager. `winget install sidebar` becomes a real install path. No cost, low effort, real reach.

**Caveat:** winget will install whatever you point it at. Point it at your SignPath-signed GitHub Releases binary for the best user experience.

### 6.5 Open Source Collective (fiscal host) — apply later

- **Direct URL:** [oscollective.org/projects](https://oscollective.org/projects/).
- **Fee:** Flat 10% on incoming funds (covers host + platform + processing).

**When to apply:** Only if/when the project receives a grant that requires a fiscal sponsor, or when you want to accept tax-deductible-ish donations through a neutral non-profit. Not needed at greenfield stage. Apply after the project has traction or when a funder requires it.

---

## 7. Programs That Don't Fit (And Why)

This is the credibility section. Several widely-cited programs look attractive but do not actually serve a Windows desktop Rust utility.

### 7.1 Vercel Open Source Program — ❌ doesn't fit

Vercel's OSS program gives **$3,600 in Vercel platform credits** to qualifying OSS projects ([Vercel blog — Spring 2026 cohort](https://vercel.com/blog/vercel-open-source-program-spring-2026-cohort)). Vercel is a **web hosting platform** (Next.js, static sites, edge functions). `sidebar` is a **desktop app** — it has no web frontend, no SSR, no edge function needs. The only conceivable use is hosting a one-page docs/landing site, which Vercel's **free Hobby tier already covers** without needing the OSS program. Apply only if you spin up a real documentation site with meaningful traffic that exceeds Hobby limits — unlikely for this project.

### 7.2 Netlify Open Source Plan — ❌ doesn't fit

Same logic as Vercel. Netlify's OSS plan ([netlify.com/open-source](https://www.netlify.com/open-source/)) provides free hosting for qualifying OSS projects — primarily for docs sites, issue trackers, and project blogs. The free tier is already generous. No application needed for a docs site; don't apply.

### 7.3 DigitalOcean Open Source Credits — ❌ doesn't fit

DO's credits are for **Droplets (VMs), Spaces (object storage), Kubernetes** ([digitalocean.com/open-source/credits-for-projects](https://www.digitalocean.com/open-source/credits-for-projects)). A desktop app has no server-side workload. There's nothing meaningful to spend these credits on. Skip.

### 7.4 AWS Promotional Credits for OSS — ❌ doesn't fit

Same as DigitalOcean. AWS credits = EC2/S3/Lambda. Desktop app has no AWS footprint. Skip.

### 7.5 Cloudflare Project Alexandria — ⚠️ marginal

Project Alexandria ([Cloudflare blog](https://blog.cloudflare.com/expanding-our-support-for-oss-projects-with-project-alexandria/)) gives recurring annual credits for **Workers, Pages, R2**. Cloudflare's **free tier is already very generous** (100K Workers requests/day, free Pages hosting, R2 free tier). The marginal value of Project Alexandria for a desktop app is near zero unless you build a non-trivial web property for the project. Low priority.

### 7.6 Rust Foundation Maintainers Fund — ❌ doesn't fit (but watch)

Launched June 2026 ([blog.rust-lang.org announcement](https://blog.rust-lang.org/2026/06/02/launching-the-rust-foundation-maintainers-fund/)). Targets **maintainers of Rust itself and any crate owned by the Rust project** — not ecosystem application developers. A solo desktop app built *in* Rust is not what this funds. Watch it in case the scope expands, but do not plan around it.

### 7.7 Rust Foundation Community Grants / Fellowships — ❌ unlikely fit

Historically funded contributors working on **Rust-language priorities** (compiler, tooling, community organizing) with ~$1,000/mo stipends. A greenfield third-party desktop application doesn't fit the "contributor to Rust" profile. Possible only if the developer separately contributes to Rust itself.

### 7.8 Sovereign Tech Fund — ❌ doesn't fit

The STF ([sovereign.tech](https://www.sovereign.tech/)) invests in **critical open source digital infrastructure** — projects like curl, ffmpeg, Scala, PHP, GNOME. €24.6M+ to 60+ projects. A greenfield solo desktop telemetry utility is not critical infrastructure. Do not apply.

### 7.9 NLnet Foundation — ⚠️ stretch

NLnet ([nlnet.nl/funding](https://nlnet.nl/funding/)) funds FOSS that benefits "the open internet." A 13th-call cycle runs April–June 2026 (€5K–€50K). A hardware telemetry sidebar is a **stretch** for "open internet" framing — you'd have to make a public-interest argument (e.g., "transparent user-side network monitoring," "open bandwidth tracking for consumer rights"). Possible but low odds. Apply only if you genuinely frame the project as a digital-rights tool.

### 7.10 NumFOCUS — ❌ doesn't fit

Fiscal sponsor for **scientific computing** (NumPy, pandas, Jupyter, Matplotlib). A hardware telemetry sidebar is not scientific software. No fit.

### 7.11 .NET Foundation — ❌ doesn't fit

Sponsorship for **.NET ecosystem** projects. Even though `OpenHardwareMonitor.exe` is C#, the `sidebar` *application* is Rust. The .NET Foundation will not sponsor a Rust app because it bundles a C# helper. No fit.

### 7.12 OpenSSF — ❌ doesn't fit

OpenSSF funds **supply-chain security of critical OSS infrastructure**. An end-user desktop app is not what they fund. No fit.

### 7.13 Software Freedom Conservancy — ❌ high bar (revisit later)

SFC ([sfconservancy.org/projects/apply](https://sfconservancy.org/projects/apply/)) is a fiscal sponsor with strong legal/infra services, but it's **selective** and prefers projects with established track records and a clear mission alignment. A greenfield solo project is unlikely to be accepted. Revisit in 1–2 years if the project has traction and you want the legal-protection services SFC provides.

### 7.14 Mozilla MOSS — ❌ DEAD

[mozilla.org/en-US/moss](https://www.mozilla.org/en-US/moss/) states explicitly: *"As a result of the 2020 restructuring at Mozilla, the MOSS program is on indefinite hiatus and is not currently accepting applications."* Do not apply. Not reviving as of 2026-07-07.

### 7.15 India-specific (MeitY, Digital India, SAMRIDH, ELEVATE) — ❌ startup-focused

India has active government funding programs (MeitY Startup Hub, SAMRIDH, Digital India Scale Up, ELEVATE) but they are all **startup/acceleration** programs requiring a registered entity, traction, and often accelerator affiliation. There is **no dedicated Indian government grant program for individual FOSS developers or OSS projects** as of 2026. Honest answer: this category does not exist in a form useful to this project. The closest India-origin funder is the private **FLOSS/fund** (Zerodha), which targets impactful existing projects — worth watching after traction.

### 7.16 Azure Trusted Signing — ⚠️ paid fallback

Not a grant. ~$120/yr. Useful only as the fallback if SignPath rejects the application. See §3.4.

---

## 8. Recommended Sequence of Actions

Ordered by dependency and ROI. Do these in sequence.

### Step 1 — Make the repo public and release-ready (Week 0–1)
- Push the Rust source to a public GitHub repo.
- Add `LICENSE` (recommend **MIT** or **MPL-2.0** — both OHM-compatible; MPL-2.0 is the safer copyleft choice for keeping the app itself open).
- Write a thorough `README.md` documenting what `sidebar` does, how to build it, and what it bundles (OHM.exe, MPL-2.0).
- Add `SECURITY.md`, `CONTRIBUTING.md`, and a `CODE_OF_CONDUCT.md`.
- Add a `funding.json` file (required for FLOSS/fund later; trivial to add now).
- Get to a tagged v0.1.0 GitHub Release with a built `sidebar.exe`.

### Step 2 — Set up free CI on GitHub Actions (Week 1)
- Public repos get **free GitHub Actions minutes** including Windows runners.
- Wire a release workflow that builds `sidebar.exe`, an MSI (via `cargo-wix`), and an MSIX package.

### Step 3 — Apply to SignPath Foundation (Week 2, after first release)
- Follow §6.1.
- Install the SignPath GitHub App, apply at signpath.org, wire the signing action.
- **In parallel:** Register a Microsoft Store individual dev account (free, ID-verified).

### Step 4 — Set up distribution channels (Week 3)
- **GitHub Releases:** Publish the SignPath-signed EXE + MSI.
- **winget:** Submit a manifest PR to `microsoft/winget-pkgs` pointing at the signed GitHub Releases binary.
- **Microsoft Store:** Submit the MSIX package through Partner Center.

### Step 5 — Set up GitHub Sponsors (Week 3)
- Self-serve; takes minutes. Wire up the donation channel for optionality.

### Step 6 — Watch list (ongoing, apply later if applicable)
- **FLOSS/fund:** Apply once the project has measurable adoption (≥ some thousands of downloads or stars). Requires `funding.json` (added in Step 1).
- **NLnet:** Only if reframing the project as a digital-rights / open-internet tool.
- **Open Source Collective:** Apply if a funder requires a fiscal sponsor, or to formalize donations.

### Step 7 — Optional paid upgrades (only if free path is insufficient)
- **Domain name** (~₹1,000–1,500/yr): Worth it for branding and the docs site. Out of pocket.
- **Azure Trusted Signing** (~$120/yr): Only if SignPath is rejected or you want a cert in your own name.

---

## 9. Maintenance & Renewal Burden

What accepting each program obligates you to:

| Program | Ongoing obligation | Renewal | Risk if dropped |
|---|---|---|---|
| **SignPath Foundation** | Per-release manual approval click; remain in compliance with Code of Conduct; MFA on all accounts | Initial verification only (one-time per their terms) | Cert revocation; future builds unsigned |
| **Microsoft Store** | Submit updates through Partner Center review | None (free, no renewal) | App delisted |
| **GitHub Actions (free for public repos)** | Keep repo public | Automatic | Lose free Windows runners if repo goes private |
| **GitHub Sponsors** | None | None | N/A |
| **winget-pkgs** | Update manifest PRs for new versions | Per-version PR | Stale version in winget |
| **Open Source Collective (if used)** | 10% fee on incoming funds; annual reporting | Annual | Account closed |
| **Azure Trusted Signing (if used)** | Pay $9.99/mo | Monthly billing | Service suspended |
| **Domain (if bought)** | Pay ~₹1,000–1,500/yr | Annual registrar renewal | Domain expires; someone else can grab it |

**The free path has near-zero ongoing burden.** SignPath requires the most discipline (per-release approval, MFA, compliance) but no financial cost.

---

## 10. Open Questions / Things to Verify at Application Time

Things that may have changed by the time you actually apply, or that this research could not fully resolve:

1. **SignPath Foundation approval rate for greenfield projects.** Their terms require the project to be "released" and "actively maintained," but they don't publish minimum traction thresholds. Verify at application time whether v0.1.0 with zero external users passes their bar, or whether they want evidence of adoption.
2. **SignPath treatment of bundled `OpenHardwareMonitor.exe`.** OHM reads low-level sensors (S.M.A.R.T., fan RPMs, CPU temps). Their "no hacking tools" clause targets vulnerability scanners — OHM is a system monitor, not a vuln scanner, so should be fine. But verify explicitly in the application and be ready to justify.
3. **Microsoft Store MSIX acceptance for hardware-telemetry apps.** Reading certain sensor data may require restricted capabilities. Verify whether `runFullTrust` is sufficient or whether the Store rejects the submission outright. Build a minimal MSIX and do a test submission early.
4. **SignPath + Rust MSIX support.** SignPath supports EXE/MSI/MSIX. Verify the GitHub Action handles Rust-built MSIX cleanly (it should, but confirm at integration time).
5. **Azure Trusted Signing individual developer geographic availability in India.** The pricing page lists India regions, but individual-developer public-preview sign-up may still be geo-restricted. Verify at the Azure portal before relying on it as a fallback.
6. **Microsoft Store free-registration rollout.** The Sept 2025 free-registration flow is still being rolled out "gradually" per Microsoft's blog. If the India flow still prompts for the $19 fee at application time, check back later or contact Partner Center support.
7. **Rust Foundation Maintainers Fund scope.** The June 2026 launch targeted Rust-project-owned crates. Watch for scope expansion to ecosystem apps — re-evaluate quarterly.
8. **FLOSS/fund threshold for "impactful."** They require demonstrable impact. Verify what metric (downloads, stars, dependents) clears the bar before applying.
9. **License choice.** MIT vs. MPL-2.0 vs. GPL-3.0 affects SignPath eligibility (all three are OSI-approved, so all clear) but affects downstream adoption and OHM (MPL-2.0) license compatibility. MPL-2.0 is the conservative choice; confirm with the architecture-agent's licensing analysis.
10. **SmartScreen reputation accrual realistic timeline.** Industry reports say "days to weeks" of real download volume. There is no published threshold. The honest answer to users is: "warnings will diminish as adoption grows." Set expectations accordingly in the README.

---

## Appendix: Sources

All URLs cited or relied upon. Retrieval date: **2026-07-07**.

### Code signing
- SignPath Foundation OSS terms — https://signpath.org/terms.html
- SignPath Foundation home — https://signpath.org/
- SignPath.io OSS solution — https://signpath.io/solutions/open-source-community
- SignPath GitHub integration docs — https://docs.signpath.io/trusted-build-systems/github
- SignPath GitHub App — https://github.com/apps/signpath
- SignPath submit-signing-request Action — https://github.com/SignPath/github-action-submit-signing-request
- Microsoft Learn — Code signing options for Windows apps — https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/code-signing-options
- Microsoft Learn — SmartScreen reputation — https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/smartscreen-reputation
- SmartScreen AppRep best practices (textslashplain) — https://textslashplain.com/2024/11/15/best-practices-for-smartscreen-apprep/
- Azure Artifact Signing pricing — https://azure.microsoft.com/en-us/pricing/details/artifact-signing/
- Azure Trusted Signing docs — https://learn.microsoft.com/en-us/azure/trusted-signing/
- Microsoft Tech Community — Trusted Signing for individual developers (Jan 2026) — https://techcommunity.microsoft.com/blog/microsoft-security-blog/trusted-signing-is-now-open-for-individual-developers-to-sign-up-in-public-previ/4273554
- r/rust — Code signing on a budget — https://www.reddit.com/r/rust/comments/1tcz2od/what_are_my_options_for_code_signing_on_a_budget/
- Real Rust+SignPath example (OpenRCT2 policy) — https://openrct2.io/code-signing-policy
- Real example (novelWriter) — https://novelwriter.io/download/code_signing.html

### Microsoft Store & distribution
- Microsoft Store developer registration — https://developer.microsoft.com/en-us/microsoft-store/register
- Windows Developer Blog — Free registration for individual developers (Sept 2025) — https://blogs.windows.com/windowsdeveloper/2025/09/10/free-developer-registration-for-individual-developers-on-microsoft-store/
- Windows Developer Blog — Free company accounts (May 2026) — https://blogs.windows.com/windowsdeveloper/2026/05/07/publish-to-microsoft-store-as-a-company-now-with-free-registration-and-faster-onboarding/
- Microsoft Learn — Free dev registration — https://learn.microsoft.com/en-us/windows/apps/publish/whats-new-individual-developer
- Microsoft Learn — Publish apps to Microsoft Store — https://learn.microsoft.com/en-us/windows/apps/publish/
- winget-pkgs repo — https://github.com/microsoft/winget-pkgs
- Microsoft Learn — WinGet — https://learn.microsoft.com/en-us/windows/package-manager/winget/

### Rust ecosystem
- Rust Foundation home — https://rustfoundation.org/
- Rust Foundation 2025 Year in Review — https://rustfoundation.org/2025/
- Rust Blog — Launching the Maintainers Fund (June 2026) — https://blog.rust-lang.org/2026/06/02/launching-the-rust-foundation-maintainers-fund/
- Rust Foundation — Announcing Maintainers Fund — https://rustfoundation.org/media/announcing-the-rust-foundation-maintainers-fund/
- Rust Foundation — Community Grants Program tag — https://rustfoundation.org/media/tag/community-grants-program/
- Rust Foundation Fellowship 2024 (Reddit) — https://www.reddit.com/r/rust/comments/1e6czrb/rust_foundation_fellowship_grants_program_2024/

### GitHub
- GitHub Sponsors — https://github.com/sponsors
- GitHub Sponsors docs — https://docs.github.com/en/sponsors/getting-started-with-github-sponsors/about-github-sponsors
- GitHub Blog — Sponsors launches in India — https://github.blog/news-insights/company-news/github-sponsors-launches-in-india/
- GitHub Accelerator — https://github.com/open-source/accelerator

### Cloud / infrastructure credits
- Vercel Open Source Program — https://vercel.com/open-source-program
- Vercel Spring 2026 cohort — https://vercel.com/blog/vercel-open-source-program-spring-2026-cohort
- Netlify Open Source — https://www.netlify.com/open-source/
- Netlify Open Source Policy — https://www.netlify.com/legal/open-source-policy/
- DigitalOcean Open Source Credits — https://www.digitalocean.com/open-source/credits-for-projects
- DigitalOcean Open Source — https://www.digitalocean.com/open-source
- Cloudflare Project Alexandria blog — https://blog.cloudflare.com/expanding-our-support-for-oss-projects-with-project-alexandria/
- Cloudflare Project Alexandria landing — https://www.cloudflare.com/lp/project-alexandria/
- AWS Promotional Credits for OSS — https://aws.amazon.com/blogs/opensource/aws-promotional-credits-open-source-projects/
- AWS Open Source — https://aws.amazon.com/opensource/
- GitLab for Open Source — https://about.gitlab.com/solutions/open-source/join/
- GitLab Community Programs docs — https://docs.gitlab.com/subscriptions/community_programs/

### Foundations & grant-makers
- Software Freedom Conservancy — Apply — https://sfconservancy.org/projects/apply/
- NLnet Foundation — Funding — https://nlnet.nl/funding/
- NLnet Commons Fund eligibility — https://nlnet.nl/commonsfund/eligibility/
- NLnet 9th-call results (April 2026) — https://nlnet.nl/news/2026/20260409-announce-commons-fund.html
- Sovereign Tech Agency — https://www.sovereign.tech/
- Sovereign Tech Agency — Programs — https://www.sovereign.tech/programs
- Mozilla MOSS (inactive) — https://www.mozilla.org/en-US/moss/
- FLOSS/fund — https://floss.fund/
- NumFOCUS — Projects overview — https://numfocus.org/projects-overview
- NumFOCUS — Programs — https://numfocus.org/programs
- Open Source Collective — Projects — https://oscollective.org/projects/
- Open Source Collective — Fees — https://docs.oscollective.org/welcome-and-introduction-to-osc/fees
- Open Collective — Pricing docs — https://documentation.opencollective.com/why-open-collective/pricing
- Open Collective — OCF dissolution statement — https://blog.opencollective.com/open-collective-official-statement-ocf-dissolution/
- OpenSSF — https://openssf.org/
- .NET Foundation — https://dotnetfoundation.org/

### India-specific
- MeitY Startup Hub — https://msh.meity.gov.in/
- Digital India — https://www.digitalindia.gov.in/
- PRS India — MeitY Budget 2026-27 analysis — https://prsindia.org/files/budget/budget_parliament/2026/DFG_Analysis_2026-27_MeITY.pdf
- FLOSS/fund (India-origin) — https://floss.fund/

---

*End of document.*
