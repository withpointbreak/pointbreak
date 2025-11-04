# Usage Guide

Learn how to use Pointbreak effectively for AI-assisted debugging.

## Basic Debugging Workflow

### 1. Set Breakpoints

Ask your AI to set breakpoints:

```
"Set a breakpoint on line 42 of main.py"
"Add a breakpoint at the start of calculate_total()"
```

### 2. Start Debugging

```
"Start debugging with these breakpoints"
"Run the debugger and pause at the first breakpoint"
```

### 3. Inspect State

When paused at a breakpoint:

```
"Show me the value of user_input"
"What are all the local variables?"
"Display the stack trace"
```

### 4. Step Through Code

```
"Step over the next line"
"Step into this function call"
"Step out of this function"
"Continue until the next breakpoint"
```

## Common Patterns

### Finding a Bug

```
User: "This function returns the wrong value. Help me find the bug."

AI: I'll set a breakpoint at the function entry and step through:
1. Setting breakpoint at line 15...
2. Starting debugger...
3. [Paused at line 15]
4. Current value of input: [1, 2, 3]
5. Stepping through...
6. Found the issue: You're dividing by zero when the list is empty.
```

### Understanding Complex Logic

```
User: "How does this recursive function work?"

AI: Let me step through a few iterations:
1. Setting breakpoint at recursion entry...
2. Starting with input: 5
3. [First call] n=5
4. [Second call] n=4
5. [Third call] n=3
...
```

### Tracking Variable Changes

```
User: "When does this variable change?"

AI: I'll set a conditional breakpoint:
"Set breakpoint on line 30 with condition: count > 10"
```

## Advanced Features

### Conditional Breakpoints

```
"Set a breakpoint on line 42, but only break when x > 100"
"Add a breakpoint that only triggers after 5 hits"
```

### Log Points

```
"Add a log point on line 30 that prints the value of total"
```

This prints without stopping execution!

### Watch Expressions

```
"Add a watch expression for user.name"
"Monitor the value of count throughout execution"
```

### Evaluate Expressions

```
"Evaluate: len(data) * 2"
"What's the result of: user.age + 10?"
```

## Language-Specific Examples

### Python

```python
def process_data(items):
    result = []
    for item in items:  # Breakpoint here
        processed = item * 2
        result.append(processed)
    return result
```

**Ask:** "Set a breakpoint in the loop and show me item values"

### Rust

```rust
fn calculate_sum(numbers: &[i32]) -> i32 {
    let mut sum = 0;  // Breakpoint here
    for &num in numbers {
        sum += num;
    }
    sum
}
```

**Ask:** "Break at the sum initialization and step through the loop"

### JavaScript

```javascript
function fetchUserData(userId) {
  const response = fetch(`/api/users/${userId}`); // Breakpoint here
  return response.json();
}
```

**Ask:** "Set a breakpoint and show me the userId value"

## Best Practices

### ✅ Do

- Be specific about line numbers
- Set breakpoints before starting the debugger
- Ask for context (stack trace, variables)
- Use natural language
- Step through unfamiliar code

### ❌ Don't

- Add console.log or print statements (use the debugger!)
- Guess at values (inspect them!)
- Run without breakpoints in complex code
- Forget to ask for the stack trace when confused

## Debugging Strategies

### Top-Down

```
1. Set breakpoint at function entry
2. Step through line by line
3. Inspect variables at each step
4. Find where behavior diverges from expected
```

### Bottom-Up

```
1. Set breakpoint where error occurs
2. Check stack trace
3. Set breakpoints up the call chain
4. Find where bad data originates
```

### Binary Search

```
1. Set breakpoint in middle of suspect code
2. Check if bug already present
3. Move breakpoint up or down
4. Repeat until bug location narrowed down
```

## Tips for AI Assistants

### Being Clear

❌ "Debug this"
✅ "Set a breakpoint on line 42, start debugging, and show me the variable values"

### Providing Context

❌ "It's broken"
✅ "This function should return 10 but returns 0. Set breakpoints and find why."

### Iterating

```
User: "Set a breakpoint at line 15"
AI: [Sets breakpoint and starts debugging]
User: "Now step into the function call"
AI: [Steps in]
User: "What's the value of result?"
AI: [Shows value]
```

## Questions?

- [Troubleshooting Guide](troubleshooting.md)
- [Getting Started](getting-started.md)
- [GitHub Discussions](https://github.com/withpointbreak/pointbreak/discussions)
