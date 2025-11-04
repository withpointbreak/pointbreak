# Privacy Policy

**Effective Date:** November 3, 2025  
**Last Updated:** November 3, 2025

## Introduction

Kevin Swiber ("we," "us," or "our") operates Pointbreak, a debugging tool that enables AI assistants to control debuggers in integrated development environments (IDEs). This Privacy Policy explains how we collect, use, and protect information when you use Pointbreak.

**Key Principle: We collect NO data from your debugging sessions.**

## What Pointbreak Is

Pointbreak is a local debugging tool that:

- Runs entirely on your computer
- Connects to your IDE's debugging interface
- Receives commands from AI assistants via the Model Context Protocol (MCP)
- Controls debug sessions locally

All debugging operations occur on your local machine. We do not have access to your code, debug sessions, or development environment.

## Information We Do NOT Collect

We want to be crystal clear about what we don't collect:

- ❌ **Source code** - We never see your code
- ❌ **Debug session data** - Stack traces, variable values, breakpoint data
- ❌ **File paths or project structure** - We don't know what you're debugging
- ❌ **IDE contents** - We don't access your editor windows or files
- ❌ **Personal information** - Name, email, or identifying information (unless you sign up for future paid features)
- ❌ **Usage logs** - We don't track which files or functions you debug

## Optional Telemetry (Opt-In Only)

In the future, Pointbreak may include **optional** telemetry that is **disabled by default**. You will have to explicitly opt-in. There is no active telemetry being communicated at this time.

### If You Opt-In, We Will Collect:

- ✅ **Anonymous usage statistics:**
  - Which Pointbreak features are used (e.g., "breakpoint set," "step over")
  - Frequency of feature usage
  - Error rates (if commands fail)
- ✅ **Technical information:**
  - Operating system and version
  - Pointbreak version
  - IDE type (VS Code, Cursor, etc.)
- ✅ **Performance metrics:**
  - Command execution time
  - MCP connection status

### We Still Will NOT Collect:

Even with telemetry enabled:

- ❌ Your source code or file contents
- ❌ Debug session data (variables, stack traces)
- ❌ File paths or names
- ❌ IP addresses or identifying information

### How We Will Use Telemetry:

- Understand which features are most valuable
- Identify and fix bugs
- Improve performance
- Prioritize new features

### Opting Out:

You can change your preference at any time. Opt-out will be the default setting.

## Third-Party Services

### AI Assistant Services

When using Pointbreak with AI assistants (Claude, etc.), your **conversations with the AI** may include information about your debugging session. This data is sent to the AI service provider, not to Pointbreak.

**What the AI might see:**

- Debugging commands you request ("set a breakpoint on line 42")
- Stack traces or errors you share with the AI
- Code snippets you discuss with the AI

**Recommended:**

- Review the privacy policy of your AI service provider:
  - Anthropic (Claude): https://www.anthropic.com/legal/privacy
  - OpenAI (ChatGPT): https://openai.com/privacy

Pointbreak itself does not send your data to AI services. The AI assistant's MCP client communicates with your local Pointbreak server.

### IDE Integrations

Pointbreak connects to your IDE's Debug Adapter Protocol (DAP) interface and/or other debug APIs provided by your IDE. All communication happens locally on your machine. No data is sent to IDE vendors (Microsoft, Cursor, etc.) by Pointbreak.

## Future Paid Features

We plan to introduce optional paid features that may require data collection:

### Planned Features:

- **CI/CD Integration** - Debug CI pipeline failures
- **Cloud Snapshot Storage** - Store and replay debug sessions
- **Team Collaboration** - Share debug sessions with teammates

### If You Use Paid Features:

We will collect only the data necessary to provide the service:

- Account information (email, name for billing)
- Debug snapshots (if you opt to store them)
- Usage data for billing purposes

**You will always have control:**

- Core local debugging remains free and private
- Paid features will be opt-in with clear data practices
- You can delete your data at any time

We will update this Privacy Policy before launching paid features.

## Data Storage and Security

### Current (Free Version):

No data is stored on our servers. Everything runs locally on your machine.

### Future (Paid Features):

If you opt into paid features with cloud storage:

- Data will be encrypted in transit (TLS)
- Data will be encrypted at rest
- Stored on secure cloud infrastructure (AWS/GCP)
- You can request deletion at any time

## Children's Privacy

Pointbreak is not intended for users under 13 years of age. We do not knowingly collect information from children.

## Your Rights

You have the right to:

- **Access** - Request what data we have (currently: none, unless you opt-in to telemetry)
- **Deletion** - Request deletion of any telemetry data
- **Opt-out** - Disable telemetry at any time
- **Portability** - Request your data in a portable format (if we have any)

To exercise these rights, email: privacy@withpointbreak.com

## International Users

Pointbreak runs locally on your computer. If you opt-in to telemetry, data may be transmitted to servers in the United States. By opting in, you consent to this transfer.

## Changes to This Policy

We may update this Privacy Policy to reflect:

- New features or services
- Changes in data practices
- Legal requirements

**How we'll notify you:**

- Post updated policy at https://withpointbreak.com/privacy
- Show notification in Pointbreak on next run (for material changes)
- Update "Last Updated" date at the top

Continued use after changes constitutes acceptance of the updated policy.

## Contact Us

Questions or concerns about privacy?

**Email:** privacy@withpointbreak.com  
**Website:** https://withpointbreak.com  
**GitHub Issues:** https://github.com/withpointbreak/pointbreak/issues

## Transparency Commitment

We believe in transparency:

- We will never sell your data
- We will never share your code or debug sessions
- We will clearly communicate any changes to data practices
- We will give you control over your data

**Our philosophy:** Debugging is private. Your code stays on your machine.

---

**Summary:**

- ✅ Pointbreak runs locally - we don't see your code
- ✅ No data collection by default
- ✅ Optional telemetry will be anonymous and opt-in, but no telemetry exists today
- ✅ AI conversations are between you and the AI service
- ✅ Future paid features will be opt-in with clear terms
