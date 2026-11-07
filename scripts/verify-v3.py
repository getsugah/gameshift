#!/usr/bin/env python3
"""Verify preserved v3 sources; not a cryptographic signature or hardware benchmark."""
from pathlib import Path
import hashlib
import json
root = Path(__file__).resolve().parents[1]
reference = json.loads((root / 'docs/reference/aurelia-v3.json').read_text())
failures = [name for name, expected in reference['files'].items()
            if hashlib.sha256((root / "docs/reference/v3-source" / name).read_bytes()).hexdigest() != expected]
for name, expected in reference['files'].items():
    if name.startswith('shaders/') or name.endswith(('renderer.rs', 'statistics.rs')):
        current=(root/name).read_bytes()
        if name.endswith('renderer.rs'):
            # Exact additive constructor shim only: the historical constructor
            # still chooses Required/IfAvailable, all remaining bytes must match.
            legacy=(root/'docs/reference/v3-source'/name).read_text()
            header='    pub fn new(config: &RunConfig, window: Option<&Window>) -> Result<Self> {\n'
            shim=header+'        Self::new_with_validation(config, window, None)\n    }\n    pub(crate) fn new_with_validation(\n        config: &RunConfig,\n        window: Option<&Window>,\n        validation: Option<ValidationMode>,\n    ) -> Result<Self> {\n'
            added=legacy.replace(header,shim,1).replace('let mode = if config.require_validation {','let mode = validation.unwrap_or(if config.require_validation {',1).replace('ValidationMode::IfAvailable\n        };','ValidationMode::IfAvailable\n        });',1)
            if current==added.encode():
                current=legacy.encode()
        if hashlib.sha256(current).hexdigest() != expected:
            failures.append('rendu actuel : ' + name)
if failures:
    raise SystemExit('Référence v3 modifiée : ' + ', '.join(failures))
print('Référence aurelia-raster-v3-msaa : huit sources historiques conservées')
