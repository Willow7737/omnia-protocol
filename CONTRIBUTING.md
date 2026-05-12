# 🤝 Contributing to Omnia Protocol

Thank you for your interest in contributing to Omnia! Whether you're a cryptographer, Rust developer, or visionary, there's a place for you.

> **⚠️ This is a Rust-only codebase.** All contributions are in Rust. There are no JavaScript, Python, or other language components.

## 📜 Code of Conduct

We are committed to providing a welcoming and inspiring community for all. Please read and adhere to our Code of Conduct:

- **Respect:** Treat all community members with respect
- **Inclusivity:** Welcome people of all backgrounds and identities
- **Collaboration:** Work together toward shared goals
- **Integrity:** Act with honesty and transparency
- **Safety:** Maintain a harassment-free environment

Violations can be reported to conduct@omnia.protocol.

---

## 🚀 Getting Started

### 1. Fork the Repository

```bash
gh repo fork Willow7737/omnia-protocol --clone
cd omnia-protocol
```

### 2. Create a Feature Branch

```bash
git checkout -b feature/your-feature-name
```

### 3. Set Up Development Environment

```bash
cargo build
cargo test --workspace
```

### 4. Make Your Changes

Follow the guidelines below for your specific contribution type.

### 5. Commit and Push

```bash
git add .
git commit -m "feat: description of your change"
git push origin feature/your-feature-name
```

### 6. Submit a Pull Request

- Create a pull request on GitHub
- Provide a clear description of your changes
- Link any related issues
- Wait for code review (1 approval for now — small team; 2+ approvals as team grows)

---

## 🛠️ Contribution Types

### 1. Code Contributions (Rust Only) 🦀

```rust
// Follow Rust conventions
// - Use clippy for linting
// - Use fmt for formatting
// - Write tests for all public functions

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_functionality() {
        // Test implementation
    }
}
```

**Requirements:**
- ✅ Tests pass: `cargo test --workspace`
- ✅ Clippy passes: `cargo clippy -- -D warnings`
- ✅ Format is correct: `cargo fmt --check`
- ✅ Documentation is complete: `cargo doc`

### 2. Documentation Contributions 📝

- Use Markdown format
- Follow the existing structure
- Include code examples where relevant
- Proofread for clarity and grammar
- **Be honest**: Label stubs as ⚠️, planned features as 📋, aspirational content as 🔮
- Do not document features that don't exist in the code

### 3. Community Contributions 🌍

- Organize meetups or webinars
- Create educational content
- Moderate community discussions
- Provide support to new members
- Translate documentation to other languages

---

## 🔍 Review Process

### Code Review Checklist

Before submitting a pull request, ensure:

- [ ] Code follows project style guide
- [ ] Tests are included and passing (`cargo test --workspace`)
- [ ] Documentation is updated
- [ ] No breaking changes (or documented)
- [ ] Commit messages are clear
- [ ] No security vulnerabilities
- [ ] Clippy passes (`cargo clippy -- -D warnings`)
- [ ] Formatting is correct (`cargo fmt --check`)

### Reviewer Responsibilities

Reviewers will:

1. **Check functionality:** Does the code do what it claims?
2. **Check quality:** Is the code well-written and maintainable?
3. **Check tests:** Are there adequate tests?
4. **Check documentation:** Is the documentation clear and honest?
5. **Check security:** Are there any security issues?

### Approval Process

| Approval Level | Meaning |
|---------------|---------|
| **1 approval** | Code review complete (current policy — small team) |
| **2+ approvals** | Ready to merge (policy when team grows) |
| **Changes requested** | Address feedback and resubmit |

---

## 📝 Commit Message Guidelines

Use clear, descriptive commit messages:

```
feat: add new feature
fix: fix bug in component
docs: update documentation
style: format code
refactor: restructure code
test: add tests
chore: update dependencies
```

**Format:**
```
<type>(<scope>): <subject>

<body>

<footer>
```

**Example:**
```
feat(consensus): implement causal graph consensus

Add support for causal graph consensus mechanism with vector clocks.
This enables parallel processing of independent transactions.

Fixes #123
```

---

## 🧪 Testing Guidelines

### Unit Tests

```rust
#[test]
fn test_basic_functionality() {
    let result = my_function(input);
    assert_eq!(result, expected);
}
```

### Integration Tests

```rust
#[test]
fn test_end_to_end_flow() {
    let system = setup_system();
    let result = system.execute_flow();
    assert!(result.is_ok());
}
```

### Test Coverage

- Aim for >80% coverage
- Test happy paths and error cases
- Test edge cases
- Test security-sensitive code thoroughly
- Run all workspace tests: `cargo test --workspace`

---

## 📖 Documentation Guidelines

### Code Documentation

```rust
/// Brief description of the function.
///
/// Longer description explaining what the function does,
/// how it works, and any important details.
///
/// # Arguments
///
/// * `arg1` - Description of arg1
/// * `arg2` - Description of arg2
///
/// # Returns
///
/// Description of return value
///
/// # Examples
///
/// ```
/// let result = my_function(arg1, arg2);
/// assert_eq!(result, expected);
/// ```
pub fn my_function(arg1: Type1, arg2: Type2) -> ReturnType {
    // Implementation
}
```

---

## 🐛 Reporting Issues

### Bug Reports

Include:
- Clear description of the bug
- Steps to reproduce
- Expected behavior
- Actual behavior
- Environment details (OS, Rust version, etc.)
- Logs if relevant

**Template:**
```markdown
## 🐛 Description
Brief description of the bug

## Steps to Reproduce
1. Step 1
2. Step 2
3. Step 3

## Expected Behavior
What should happen

## Actual Behavior
What actually happens

## Environment
- OS: macOS 12.0
- Rust: 1.75.0
- Version: 1.0.0
```

### Feature Requests

Include:
- Clear description of the feature
- Use case and motivation
- Proposed implementation (if any)
- Alternative approaches considered

---

## 💬 Communication

Omnia is a public-interest protocol. Join the conversation:

- **[GitHub Discussions](https://github.com/Willow7737/omnia-protocol/discussions)** - Questions, ideas, and general community interaction.
- **[GitHub Issues](https://github.com/Willow7737/omnia-protocol/issues)** - Bug reports, feature requests, and technical research proposals.
- **[Project Dashboard](./PROJECT_DASHBOARD.md)** - Real-time project health and status updates.
- **[Discord](https://discord.gg/qYkpAeSYR)** - Real-time chat and community.
- **Email:** `conduct@omnia.protocol` (for conduct issues)

### Response Times

| Type | Expected Response |
|------|-------------------|
| 🐛 Bug reports | 24-48 hours |
| 💡 Feature requests | 1 week |
| 🔀 Pull requests | 2-5 days |
| ❓ Questions | 24 hours |

---

## 📜 License

By contributing to Omnia Protocol, you agree that your contributions will be licensed under the same license as the project (CC0 Public Domain).

---

**Last Updated:** May 2026
