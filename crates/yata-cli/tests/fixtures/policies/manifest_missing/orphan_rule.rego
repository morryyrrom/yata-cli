package yata.rules.cli_missing_policies.orphan_rule

import rego.v1

metadata := {
	"title": "A rule in a directory without a manifest",
	"severity": "low",
	"description": "The directory has no yata-policies.json.",
	"remediation": "None.",
	"target_type": "aws.account.account",
	"references": [],
}

default fail := false

fail if input.id

message := "never evaluated"
