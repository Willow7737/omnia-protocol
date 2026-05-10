# Contributing to Omnia Protocol

Thank you for your interest in contributing to Omnia! Whether you're a cryptographer, developer, designer, economist, or visionary, there's a place for you.

## Code of Conduct

We are committed to providing a welcoming and inspiring community for all. Please read and adhere to our Code of Conduct:

- **Respect:** Treat all community members with respect
- **Inclusivity:** Welcome people of all backgrounds and identities
- **Collaboration:** Work together toward shared goals
- **Integrity:** Act with honesty and transparency
- **Safety:** Maintain a harassment-free environment

Violations can be reported to conduct@omnia.protocol.

---

## Getting Started

### 1. Fork the Repository

```bash
gh repo fork omnia-protocol/omnia-protocol --clone
cd omnia-protocol
```

### 2. Create a Feature Branch

```bash
git checkout -b feature/your-feature-name
```

### 3. Set Up Development Environment

```bash
# Install dependencies
pnpm install

# Build the project
pnpm build

# Run tests
pnpm test
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
- Wait for code review (2+ approvals required)

---

## Contribution Types

### 1. Code Contributions

#### For Rust Code

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
- Tests pass: `cargo test`
- Clippy passes: `cargo clippy -- -D warnings`
- Format is correct: `cargo fmt`
- Documentation is complete: `cargo doc`

#### For JavaScript/TypeScript Code

```typescript
// Follow TypeScript conventions
// - Use eslint for linting
// - Use prettier for formatting
// - Write tests for all exported functions

export function myFunction(input: string): string {
  // Implementation
}

describe('myFunction', () => {
  it('should work correctly', () => {
    expect(myFunction('test')).toBe('result');
  });
});
```

**Requirements:**
- Tests pass: `pnpm test`
- ESLint passes: `pnpm lint`
- Format is correct: `pnpm format`
- Types are correct: `pnpm type-check`

### 2. Documentation Contributions

#### Writing Documentation

- Use Markdown format
- Follow the existing structure
- Include code examples where relevant
- Proofread for clarity and grammar

#### Documentation Standards

```markdown
# Section Title

Brief introduction to the section.

## Subsection

More detailed explanation with examples.

### Code Example

\`\`\`rust
// Code example
\`\`\`

### Key Points

- Point 1
- Point 2
- Point 3
```

### 3. Design Contributions

#### UI/UX Design

- Submit designs as high-fidelity mockups
- Include interaction flows
- Provide design rationale
- Consider accessibility (WCAG 2.1 AA)

#### Visual Assets

- Submit as SVG or PNG (high resolution)
- Include source files (Figma, Adobe XD)
- Provide usage guidelines
- Ensure consistency with design system

### 4. Research Contributions

#### Writing Research Papers

- Submit as PDF or Markdown
- Include abstract, introduction, methodology, results, conclusion
- Cite sources properly
- Provide reproducible results

#### Sharing Research

- Post in the research forum
- Link to published papers
- Engage in peer review
- Contribute to knowledge base

### 5. Community Contributions

#### Event Organization

- Organize meetups or webinars
- Create educational content
- Moderate community discussions
- Provide support to new members

#### Translations

- Translate documentation to other languages
- Ensure accuracy and consistency
- Maintain translation glossary
- Update translations as docs change

---

## Review Process

### Code Review Checklist

Before submitting a pull request, ensure:

- [ ] Code follows project style guide
- [ ] Tests are included and passing
- [ ] Documentation is updated
- [ ] No breaking changes (or documented)
- [ ] Commit messages are clear
- [ ] No security vulnerabilities

### Reviewer Responsibilities

Reviewers will:

1. **Check functionality:** Does the code do what it claims?
2. **Check quality:** Is the code well-written and maintainable?
3. **Check tests:** Are there adequate tests?
4. **Check documentation:** Is the documentation clear?
5. **Check security:** Are there any security issues?

### Approval Process

- **1 approval:** Code review complete
- **2+ approvals:** Ready to merge
- **Changes requested:** Address feedback and resubmit

---

## Commit Message Guidelines

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

## Testing Guidelines

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

---

## Documentation Guidelines

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

### README Files

- Include project overview
- Provide quick start guide
- Link to detailed documentation
- Include examples
- List dependencies

### API Documentation

- Document all endpoints
- Include request/response examples
- Explain error codes
- Provide authentication details

---

## Reporting Issues

### Bug Reports

Include:
- Clear description of the bug
- Steps to reproduce
- Expected behavior
- Actual behavior
- Environment details (OS, version, etc.)
- Screenshots or logs if relevant

**Template:**
```markdown
## Description
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
- Node: 16.0.0
- Version: 1.0.0
```

### Feature Requests

Include:
- Clear description of the feature
- Use case and motivation
- Proposed implementation (if any)
- Alternative approaches considered

---

## Recognition & Rewards

### Contributor Recognition

- Listed in CONTRIBUTORS.md
- GitHub profile linked
- Social media shoutout
- Monthly community newsletter

### RPGF Rewards

Contributors can earn RPGF rewards:

| Contribution | Reward |
|--------------|--------|
| Merged PR (code) | 100-1,000 Omnia |
| Merged PR (docs) | 50-500 Omnia |
| Research paper | 500-5,000 Omnia |
| Community event | 200-2,000 Omnia |
| Translation | 100-500 Omnia |

**Process:**
1. Contribute and get merged
2. Apply for RPGF funding
3. Community votes on reward
4. Funds distributed automatically

---

## Development Workflow

### Setting Up Your Environment

```bash
# Clone the repository
gh repo clone omnia-protocol/omnia-protocol

# Install dependencies
cd omnia-protocol
pnpm install

# Create a feature branch
git checkout -b feature/your-feature

# Make changes
# ...

# Run tests
pnpm test

# Format code
pnpm format

# Commit and push
git add .
git commit -m "feat: your feature"
git push origin feature/your-feature
```

### Continuous Integration

All pull requests run through CI:

- Tests must pass
- Linting must pass
- Coverage must not decrease
- Security checks must pass

---

## Communication

### Where to Ask Questions

Omnia is a public-interest protocol. Join the conversation across our various channels:

- **[GitHub Discussions](https://github.com/Willow7737/omnia-protocol/discussions)** - The primary place for questions, ideas, and general community interaction.
- **[GitHub Issues](https://github.com/Willow7737/omnia-protocol/issues)** - For bug reports, feature requests, and technical research proposals.
- **[Project Dashboard](./PROJECT_DASHBOARD.md)** - For real-time project health and status updates.
- **Discord:** Real-time chat and community [Join our Discord](https://discord.gg/qYkpAeSYR)
- **Forum:** Detailed discussions and proposals (Link coming soon)
- **Email:** `conduct@omnia.protocol` (for conduct issues)

### Response Times

- **Bug reports:** 24-48 hours
- **Feature requests:** 1 week
- **Pull requests:** 2-5 days
- **Questions:** 24 hours

---

## License

By contributing to Omnia Protocol, you agree that your contributions will be licensed under the same license as the project (CC0 Public Domain).

---

## Thank You

Thank you for contributing to Omnia! Together, we're building the infrastructure for a better future.

---

**Last Updated:** May 2026
