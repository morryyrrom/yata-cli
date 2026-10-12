package yata.rules.eval_policies.fail_not_set

import rego.v1

metadata := {
	"title": "fail is a set only without widgets",
	"severity": "low",
	"description": "A complete rule that is defined (as a set) only when there is no widget.",
	"remediation": "None.",
	"target_type": "aws.example.widget",
	"references": [],
}

# Defined without input (so loading accepts it), undefined once a widget exists.
fail := set() if {
	not input.aws.example.widget[0]
}
