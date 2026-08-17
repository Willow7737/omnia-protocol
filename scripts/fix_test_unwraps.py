#!/usr/bin/env python3
"""Replace .unwrap() and .unwrap_err() ONLY inside #[cfg(test)] blocks.

Strategy:
1. For files in */tests/*.rs — replace everywhere in the file
2. For src/*.rs files — only replace inside #[cfg(test)] mod blocks
"""
import re

def process_test_file(filepath):
    """Files under tests/ directories — replace all unwraps."""
    with open(filepath) as f:
        content = f.read()
    
    original = content
    content = re.sub(r'\.unwrap_err\(\)', '.expect_err("test assertion failed")', content)
    content = re.sub(r'\.unwrap\(\)', '.expect("test assertion failed")', content)
    
    if content != original:
        with open(filepath, 'w') as f:
            f.write(content)
        return content.count('.expect(') - original.count('.expect(')
    return 0

def process_src_test_blocks(filepath):
    """src/ files — only replace inside #[cfg(test)] mod blocks."""
    with open(filepath) as f:
        content = f.read()
    
    original = content
    
    # Find all #[cfg(test)] mod test { ... } blocks and process them
    # This regex finds cfg(test) modules
    pattern = r'(#\[cfg\(test\)\]\s*(?:#\[allow\([^)]*\)\]\s*)*mod\s+\w+\s*\{)'
    
    parts = []
    last_end = 0
    
    for m in re.finditer(pattern, content):
        start = m.start()
        # Add content before this test module unchanged
        parts.append(content[last_end:start])
        
        # Find the matching closing brace
        brace_start = content.index('{', start)
        depth = 0
        i = brace_start
        while i < len(content):
            if content[i] == '{':
                depth += 1
            elif content[i] == '}':
                depth -= 1
                if depth == 0:
                    break
            i += 1
        
        # Extract the test module content (including the #[cfg(test)] mod ... { part)
        module_content = content[start:i+1]
        
        # Replace unwraps only within this module
        module_content = re.sub(r'\.unwrap_err\(\)', '.expect_err("test assertion failed")', module_content)
        module_content = re.sub(r'\.unwrap\(\)', '.expect("test assertion failed")', module_content)
        
        parts.append(module_content)
        last_end = i + 1
    
    parts.append(content[last_end:])
    result = ''.join(parts)
    
    if result != original:
        with open(filepath, 'w') as f:
            f.write(result)
        return result.count('.expect(') - original.count('.expect(')
    return 0

import os, sys

base = "/home/z/my-project/omnia-protocol"

# Test directories (replace all)
test_dirs = [
    "fee-burn/tests",
    "asset-registry/tests", 
    "payment-order/tests",
    "node/tests",
    "tests",
]

# Src directories (only cfg(test) blocks)
src_dirs = [
    "fee-burn/src",
    "asset-registry/src",
    "payment-order/src",
    "shards/src",
    "node/src",
]

total = 0

for d in test_dirs:
    full = os.path.join(base, d)
    if not os.path.isdir(full):
        continue
    for root, dirs, files in os.walk(full):
        for f in files:
            if f.endswith('.rs'):
                fp = os.path.join(root, f)
                n = process_test_file(fp)
                if n:
                    print(f"  {os.path.relpath(fp, base)}: {n}")
                    total += n

for d in src_dirs:
    full = os.path.join(base, d)
    if not os.path.isdir(full):
        continue
    for root, dirs, files in os.walk(full):
        for f in files:
            if f.endswith('.rs'):
                fp = os.path.join(root, f)
                n = process_src_test_blocks(fp)
                if n:
                    print(f"  {os.path.relpath(fp, base)}: {n}")
                    total += n

print(f"Total: {total} replacements")
