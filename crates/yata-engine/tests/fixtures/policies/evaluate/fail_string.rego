package yata.rules.eval_policies.fail_string

import rego.v1

metadata := {
	"title": "fail is a string once a widget exists",
	"severity": "low",
	"description": "A set without widgets, a non-set value with a widget.",
	"remediation": "None.",
	"target_type": "aws.example.widget",
	"references": [],
}

fail := set() if {
	not input.aws.example.widget[0]
} else := "x"
