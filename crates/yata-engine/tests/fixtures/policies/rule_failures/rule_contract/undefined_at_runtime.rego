package yata.rules.test_policies.rule_contract.undefined_at_runtime

import rego.v1

metadata := {
	"title": "Rule contract violation: undefined_at_runtime",
	"severity": "medium",
	"description": "Has no default, so fail is undefined for a widget with a flag. Without input the body holds, so loading accepts it.",
	"remediation": "Nothing to fix.",
	"target_type": "aws.example.widget",
	"references": [],
}

# Without input `not input.tags.flag` holds; for a widget that has a flag the body fails and `fail` is undefined.
fail if not input.tags.flag
