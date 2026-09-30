# OpsState v2 contract

This source-first series restricts updates to the exact integer range shared by Native and Workers. The v1 series remains separate; existing v1 readers do not silently adopt v2. Ordinary `lenso app build` generates and checks the projections.
