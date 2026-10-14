package yata.rules.eval_policies.title_only

import rego.v1

metadata := {
	"title": "Widget must not be flagged",
	"severity": "medium",
	"description": "Has neither message nor reason, so the title is used.",
	"remediation": "Clear the flag.",
	"target_type": "aws.example.widget",
	"references": [],
}

default fail := false

fail if input.tags.flag == "bad"
