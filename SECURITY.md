# Security Policy

## Our Commitment

Security is critical for Pointbreak. As a debugging tool that accesses your IDE and code, security is taken seriously, and the security community's help in keeping Pointbreak safe is appreciated.

## Supported Versions

| Version | Supported          |
| ------- | ------------------ |
| 0.x.x   | :white_check_mark: |

**Note:** Pointbreak is currently in beta (0.x.x versions). Please update to the latest version before reporting issues.

## Reporting a Vulnerability

**🚨 DO NOT open public GitHub issues for security vulnerabilities.**

Public disclosure of security issues puts all users at risk. Instead:

### How to Report

**Email:** security@withpointbreak.com

**Subject line:** "Security Vulnerability in Pointbreak"

### What to Include

Please provide:

1. **Description** - What's the vulnerability?
2. **Impact** - What can an attacker do?
3. **Steps to Reproduce** - How did you find it?
4. **Affected Versions** - Which versions are vulnerable?
5. **Suggested Fix** - (Optional) How might we fix it?
6. **Your Details** - Name/handle for credit (optional)

**Example Report:**

```
Subject: Security Vulnerability in Pointbreak

Description:
Pointbreak MCP server accepts unauthenticated connections allowing
arbitrary debug commands from any process on the system.

Impact:
Local attacker could connect to MCP server and control debug sessions,
potentially executing code in the context of the debugged process.

Steps to Reproduce:
1. Start Pointbreak MCP server
2. From separate process, connect to MCP socket
3. Send arbitrary debug commands
4. Commands execute without authentication

Affected Versions: 0.1.0 - 0.2.5

Suggested Fix:
Add authentication token requirement for MCP connections

Contact: @security_researcher (prefer anonymous credit)
```

## Response Process

Security reports are treated seriously and will responded to promptly:

### Timeline

- **Within 48 hours:** Acknowledge your report
- **Within 1 week:** Provide initial assessment and timeline
- **Within 30 days:** Release fix or provide detailed plan
- **After fix:** Public disclosure (coordinated with you)

### Process

1. **Acknowledge** - Confirm we received your report
2. **Investigate** - Assess severity and impact
3. **Develop Fix** - Create and test a patch
4. **Release** - Deploy fix in new version
5. **Disclose** - Publish security advisory
6. **Credit** - Thank you publicly (if you want)

## Severity Levels

Vulnerabilities are assessed using these levels:

### Critical 🔴

- Remote code execution
- Arbitrary file read/write outside project
- Authentication bypass in paid features
- Data exfiltration of code/credentials

**Response goal:** Patch within 7 days

### High 🟠

- Local privilege escalation
- Unauthorized debug session access
- MCP protocol bypass
- IDE crash or data loss

**Response goal:** Patch within 14 days

### Medium 🟡

- Information disclosure (non-sensitive)
- Denial of service (local)
- Debug session hijacking

**Response goal:** Patch within 30 days

### Low 🟢

- UI spoofing
- Error message information leakage
- Minor security improvements

**Response goal:** Patch in next release

## What's Considered a Security Issue

**IN SCOPE:** ✅

- **Code execution vulnerabilities**
  - RCE via MCP protocol
  - Arbitrary code in debug context
- **Authentication/Authorization issues**
  - Bypassing session controls
  - Unauthorized debug access
- **Data exposure**
  - Leaking code or credentials
  - Exposing debug session data
- **Injection attacks**
  - Command injection
  - Path traversal
- **MCP protocol vulnerabilities**
  - Protocol bypass
  - Unauthenticated access
- **IDE integration exploits**
  - Escaping sandbox
  - Cross-session attacks

**OUT OF SCOPE:** ❌

- **Social engineering** (not a technical bug)
- **Physical access attacks** (requires local access)
- **Denial of service** (user can just restart)
- **Issues in third-party services** (report to them)
- **Known issues in dependencies** (we'll upgrade)
- **Theoretical vulnerabilities** (no working exploit)
- **Beta software bugs** (use GitHub issues)

**When in doubt, report it!** It's better to evaluate a non-issue than miss a real vulnerability.

## Safe Harbor

We consider security research conducted according to this policy to be:

- ✅ **Authorized** under the Computer Fraud and Abuse Act
- ✅ **Exempt** from DMCA anti-circumvention provisions
- ✅ **Lawful** and conducted in good faith

**We will not pursue legal action** against security researchers who:

- Follow this responsible disclosure policy
- Don't access user data beyond what's needed to demonstrate the vulnerability
- Don't intentionally harm users or our systems
- Don't publicly disclose before we've patched
- Act in good faith

## What We Ask From You

**Please:**

- ✅ Give us reasonable time to fix before public disclosure
- ✅ Don't access user data beyond proof-of-concept
- ✅ Don't harm users or our services
- ✅ Don't use vulnerabilities maliciously
- ✅ Follow responsible disclosure practices

**Don't:**

- ❌ Publicly disclose before it's patched
- ❌ Access other users' debug sessions or data
- ❌ Perform denial of service attacks
- ❌ Demand payment (no bounties currently)
- ❌ Violate laws in your research

## Recognition

We believe in recognizing security researchers:

### What We Offer

**Currently:**

- 🏆 Public recognition
- 🎖️ Listed in Security Hall of Fame
- 📢 Mention in release notes
- 💜 Eternal gratitude

**Future (potentially):**

- 💰 Bug bounties
- 🎁 Free premium subscriptions
- 👕 Swag and merchandise

## Security Best Practices for Users

### For Developers Using Pointbreak

- ✅ Keep Pointbreak updated to the latest version
- ✅ Only install from official sources (npm, VS Code marketplace)
- ✅ Review MCP server permissions
- ✅ Don't share debug sessions with untrusted parties
- ✅ Be careful debugging untrusted code
- ✅ Use security features in your IDE

### For Organizations

- ✅ Audit Pointbreak before deploying internally
- ✅ Monitor for security updates
- ✅ Restrict MCP server network access
- ✅ Follow your organization's security policies
- ✅ Consider security implications of AI assistant access

## Security Features in Pointbreak

**Current protections:**

- 🔒 MCP server runs locally (not exposed to internet)
- 🔒 No remote code execution by default
- 🔒 Respects IDE security boundaries
- 🔒 No persistent storage of debug data
- 🔒 Minimal telemetry (opt-in only)

**Planned protections:**

- 🔐 MCP connection authentication
- 🔐 Signed releases (code signing)
- 🔐 Integrity verification
- 🔐 Session isolation
- 🔐 Audit logging

## Keeping Informed

**Subscribe to security updates:**

- 📧 **Email:** security-announce@withpointbreak.com (coming soon)
- 📰 **GitHub:** Watch releases for security tags
- 🐦 **Twitter:** @withpointbreak (security announcements)
- 📝 **Blog:** https://withpointbreak.com/blog

**Security advisories will be posted at:**

- GitHub Security Advisories
- Release notes (for each patched version)
- Our blog (for major issues)

## Contact

**For security issues:**

- 📧 security@withpointbreak.com
- 🔐 PGP Key: (coming soon)

**For other concerns:**

- General: legal@withpointbreak.com
- Privacy: privacy@withpointbreak.com
- Support: https://github.com/withpointbreak/pointbreak/issues

## Additional Resources

- **Privacy Policy:** https://withpointbreak.com/privacy
- **Terms of Service:** https://withpointbreak.com/terms
- **Documentation:** https://github.com/withpointbreak/pointbreak
- **Report Non-Security Bugs:** https://github.com/withpointbreak/pointbreak/issues

---

## Quick Reference

**Found a security issue?**

1. ✉️ Email: security@withpointbreak.com
2. 🤐 Don't post publicly
3. 📋 Include detailed reproduction steps
4. ⏱️ We'll respond within 48 hours
5. 🏆 We'll credit you (if you want)

**Thank you for helping keep Pointbreak secure!**

---

**Last Updated:** November 3, 2025

_This security policy is inspired by industry best practices from GitHub, HackerOne, and the security community._
