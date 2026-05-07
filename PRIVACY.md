+++
id = "8d3b36a7-9957-4f69-8e20-6f242bdfd2fe"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Privacy Policy

**Effective date:** March 2026  
**Applies to:** The Omegon software and the omegon.styrene.io website  
**Controller:** Black Meridian, LLC

---

## Short version

Omegon is a local tool. **We do not collect, store, or process any personal data from your use of the Omegon binary.** There is no telemetry, no usage reporting, and no account system.

---

## 1. The Omegon Binary

When you run Omegon on your machine:

- **No data is sent to Black Meridian.** Omegon does not include analytics, crash reporting, telemetry, or any form of phone-home functionality.
- **API keys and credentials stay on your machine.** Credentials you configure are stored in your operating system's keychain or a local secrets file (`~/.config/omegon/` or equivalent). They are never transmitted to Black Meridian.
- **Inference API calls go directly to your chosen provider.** When Omegon sends a prompt to Anthropic, OpenAI, Ollama, or another configured provider, that request goes directly from your machine to that provider's API endpoint. Black Meridian is not an intermediary and does not see the content of those requests.
- **Project memory is stored locally.** All memory facts, design tree data, session logs, and project state are stored in a local SQLite database on your machine. Nothing in this database is synced to Black Meridian.

### Inference Providers

When you configure and use an inference provider through Omegon, that provider's own privacy policy and terms of service govern how they handle your data. We have no control over and take no responsibility for provider data practices. Review the policies for the providers you use:

- Anthropic: https://www.anthropic.com/legal/privacy
- OpenAI: https://openai.com/policies/privacy-policy
- Other providers: see their respective documentation

---

## 2. The Omegon Websites

The Omegon websites are static sites served from our own infrastructure. We do not use third-party analytics services, tracking pixels, advertising networks, or social media embeds.

**What our web server logs:** Standard HTTP access logs — IP address, timestamp, requested path, user agent, and referrer. These logs are used for infrastructure monitoring and debugging. They are not sold, shared with third parties, or used for profiling. Logs are retained for a limited period consistent with operational needs.

**No cookies.** The website does not set any cookies.

**No forms or accounts.** The website does not collect names, email addresses, or any other personal information through forms.

**Fonts.** The website loads fonts from Google Fonts (fonts.googleapis.com). This is a request from your browser to Google's servers. If you prefer to avoid this, use a browser extension that blocks font CDN requests.

**GitHub.** Install links (`curl ... | sh`) and source links point to GitHub. If you access these, GitHub's privacy policy applies: https://docs.github.com/en/site-policy/privacy-policies/github-general-privacy-statement

---

## 3. Your Rights (GDPR / CCPA)

Because we do not collect personal data through the Omegon binary, there is effectively no data to request, correct, or delete from us in that context.

For web server access logs (which may contain your IP address), you may contact us at **admin@styrene.io** to inquire about data we hold. We will respond within 30 days.

If you are in the European Economic Area, you have rights under GDPR including: access, rectification, erasure, restriction, portability, and objection. The lawful basis for processing web server logs is our legitimate interest in operating and securing the website.

---

## 4. Children

Omegon is not directed at or intended for use by children under 13 (or the applicable age in your jurisdiction). We do not knowingly collect data from children.

---

## 5. Changes to This Policy

If this policy changes materially, we will update the effective date above and note the change in the [repository](https://github.com/styrene-lab/omegon/blob/main/PRIVACY.md).

---

## 6. Contact

General inquiries: **admin@styrene.io**  
GitHub: [github.com/styrene-lab/omegon](https://github.com/styrene-lab/omegon)
