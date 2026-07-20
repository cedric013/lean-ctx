# OCLA-Umbau-Ziel

Dieses Dokument ist die Fortschrittstabelle für die OCLA-Phasen und trennt
fertige Verträge von noch fehlender Produktions-Adoption.

## Fortschritt pro P-Phase

| Phase | Ziel | Stand auf `main` | Evidenz / nächster Schritt |
| --- | --- | --- | --- |
| P0 | IST-Hygiene | Erledigt | `e379c9db0`; Grundlagen bereinigt. |
| P1 | Foundation, Contracts, Ledger Evidence, Lineage | Erledigt | `#1053`, `79290e63d`. |
| P2 | OclaBus Event-Backbone | Erledigt | `1029229a1`; globaler Bus mit bounded/no-op-Modus. |
| P3 | Builtin-Traits | Erledigt | `3ce705774`; durch `#1065` auf alle 14 Builtins und Registry erweitert. |
| P4 | Trait-Adoption in Runtime | In Arbeit: 6/14 | `#1070`, `#1071`, `#1073`, `#1076`, `#1092`, `#1093`; 8 Registry-only. |
| P5 | Separater OCLA-Meilenstein | Nicht belegt | Kein eigenständiger P5-OCLA-Commit im aktuellen Verlauf; nicht als erledigt behaupten. |
| P6 | Separater OCLA-Meilenstein | Nicht belegt | Kein eigenständiger P6-OCLA-Commit im aktuellen Verlauf; Scope klären. |
| P7 | Wire Protocol, SDKs, gRPC, Contract Suite | Erledigt | `f5c447a63`; öffentliche OCLA-v1-Verifikation vorhanden. |
| P8 | Intent-/Model-Router | Implementiert, Adoption offen | `6b109c739`; Builtin vorhanden, produktiver Registry-Aufruf fehlt. |
| P9 | Response Optimizer | Implementiert, Adoption offen | `6136ca554`; Builtin vorhanden, produktiver Registry-Aufruf fehlt. |
| P10 | Separater OCLA-Meilenstein | Nicht belegt | Kein eigenständiger P10-OCLA-Commit im aktuellen Verlauf; Scope klären. |
| P11 | Agent Gateway und Deployment Surface | Vertrag/Module erledigt, Adoption offen | `40f3f97a1`; `BuiltinAgentGateway` noch nicht am A2A-Ingress verdrahtet. |

## Produktions-Adoption: aktueller Zähler

| Verdrahtet | Registry-only | Gesamt |
| ---: | ---: | ---: |
| 6 | 8 | 14 |

Verdrahtete Pfade: `CompressionProvider`, `EfficiencyAnalyzer`,
`ObservationHook`, `OutcomeTracker`, `MetricsExporter`, `UsageSink`.
Registry-only: `AgentGateway`, `ConfigTuner`, `ConnectorScheduler`,
`ExperimentRunner`, `IntentClassifier`, `ModelRouter`, `ResponseOptimizer`,
`SavingsLedger`.

## Gemergte OCLA-Änderungen

- `#1053` — P1 Foundation.
- `#1065` — Trait-Adoption-Grundlage: 14 Builtins plus Registry.
- `#1070` — UsageSink und EfficiencyAnalyzer produktiv.
- `#1071` — ObservationHook produktiv.
- `#1073` — OutcomeTracker produktiv.
- `#1075` — CompressionContentPort mit PathJail/BLAKE3.
- `#1076` — echte CompressionProvider-Kompression.
- `#1083` — fail-closed Provider und TOCTOU-Härtung.
- `#1092` — Projektwurzel und Runtime-Callsite korrigiert.
- `#1093` — MetricsExporter produktiv.

## Abschlusskriterien

Der Umbau ist erst abgeschlossen, wenn alle 14 Builtins einen belegten
Produktionsaufrufer besitzen, jeder Pfad Fehler fail-closed behandelt und die
Legacy-Pipelines weder doppelt buchen noch durch OCLA-Feedback verändert
werden. Der OSS/Private-Boundary-Audit muss vor dem Merge sauber sein.
