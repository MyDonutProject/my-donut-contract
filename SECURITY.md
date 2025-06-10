# Security Policy for DONUT Referral Matrix System

## Reporting a Vulnerability

If you discover a security vulnerability in our smart contract, please report it through one of the following channels:

- **Email**: [dev@mydonut.io](mailto:dev@mydonut.io)
- **WhatsApp**: +55123456789

When reporting, please include:
- A detailed description of the vulnerability
- Steps to reproduce the issue
- Potential impact assessment
- Any suggestions for remediation if available

## Bug Bounty Program

We offer rewards for critical security vulnerabilities found in our smart contract, based on severity:

| Severity | Description | Potential Reward (SOL) |
|----------|-------------|-------------------|
| Critical | Vulnerabilities that allow direct theft of funds, permanent freezing of funds, unauthorized control of the protocol, or exploitation of Meteora pool interactions | 30-50 SOL |
| High | Vulnerabilities that could potentially lead to loss of funds under specific conditions, Chainlink oracle manipulation, or compromise of referral matrices | 10-20 SOL |
| Medium | Vulnerabilities that don't directly threaten assets but could compromise system integrity or manipulate the upline structure | 2-5 SOL |
| Low | Vulnerabilities that don't pose a significant risk but should be addressed | 0.5-1 SOL |

The final reward amount is determined at our discretion based on:
- The potential impact of the vulnerability
- The quality of the vulnerability report
- The uniqueness of the finding
- The clarity of proof-of-concept provided

## Eligibility Requirements

A vulnerability is eligible for reward if:
- It is previously unreported
- It affects the latest version of our contract
- The reporter provides sufficient information to reproduce and fix the issue
- The reporter allows a reasonable time for remediation before public disclosure

## Scope

This security policy covers the DONUT Referral Matrix System smart contract deployed at `DegWuGAfwzMFK5BWC33q6XW7Ebk7fesZShuaXh2HveHS`.

Our scope specifically includes:
- Main contract logic (lib.rs)
- Chainlink oracle interactions
- Meteora pool interactions
- 3x1 referral matrix logic
- Address and account validations
- Token handling functions
- Deposit and payment operations
- Minting and rate control functions

## Out of Scope

The following are considered out of scope:
- Vulnerabilities in third-party applications or websites
- Vulnerabilities requiring physical access to a user's device
- Social engineering attacks
- DoS attacks requiring excessive resources
- Issues related to frontend applications rather than the smart contract itself
- Issues in third-party contracts (Meteora, Chainlink, etc.) not directly related to our integration
- Vulnerabilities in previous or undeployed versions of the contract

## Responsible Disclosure

We are committed to working with security researchers to verify and address any potential vulnerabilities reported. We request that:

1. You give us reasonable time to investigate and address the vulnerability before any public disclosure
2. You make a good faith effort to avoid privacy violations, data destruction, and interruption or degradation of our services
3. You do not exploit the vulnerability beyond what is necessary to prove it exists

## Acknowledgments

We thank all security researchers who contribute to the security of our protocol. Contributors who discover valid vulnerabilities will be acknowledged (if desired) once the issue has been resolved.

---

This document was last updated: $(date +"%B %Y")
