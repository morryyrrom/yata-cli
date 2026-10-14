package yata.rules.eval_policies.bad.undefined_at_runtime

import rego.v1

metadata := {
	"title": "Rule contract violation: undefined_at_runtime",
	"severity": "low",
	"description": "Leaves fail undefined for every widget; without input it holds, so loading accepts it.",
	"remediation": "None.",
	"target_type": "aws.example.widget",
	"references": [],
}

# Without input `not input.id` holds, so loading sees a boolean; with a widget `fail` is undefined.
fail if not input.id
