//! Relative indices, fixed weights and conservative confirmation notices.
use crate::session::{Metric, Session};
use serde::Serialize;
use std::collections::BTreeMap;
pub const SCORE_VERSION: &str = "aurelia-relative-cpu-cache-glibc-v1";
#[derive(Debug, Serialize)]
pub struct Difference {
    pub id: String,
    pub label: String,
    pub category: String,
    pub candidate_index: f64,
    pub reference_rate: f64,
    pub candidate_rate: f64,
    pub unit: String,
    pub reference_cv: f64,
    pub candidate_cv: f64,
    pub confirmation_required: bool,
}
#[derive(Debug, Serialize)]
pub struct Category {
    pub id: String,
    pub label: String,
    pub candidate_index: f64,
    pub confirmation_required: bool,
}
#[derive(Debug, Serialize)]
pub struct Comparison {
    pub score_version: String,
    pub reference: String,
    pub candidate: String,
    pub categories: Vec<Category>,
    pub native_partial_index: f64,
    pub application_index: f64,
    pub synthesis_index: f64,
    pub confirmation_required: bool,
    pub differences: Vec<Difference>,
    pub software_differences: Vec<String>,
    pub warnings: Vec<String>,
}
fn geo(values: impl Iterator<Item = f64>) -> f64 {
    let values: Vec<_> = values.collect();
    (values.iter().map(|v| v.ln()).sum::<f64>() / values.len() as f64).exp()
}
fn hardware(a: &Session, b: &Session) -> Result<(), String> {
    let a = &a.fingerprint;
    let b = &b.fingerprint;
    if a.architecture != b.architecture
        || a.cpu_model != b.cpu_model
        || a.logical_cpus != b.logical_cpus
        || a.physical_cores != b.physical_cores
        || a.pci_gpus != b.pci_gpus
    {
        return Err("Signatures matérielles différentes : comparaison refusée.".into());
    }
    let (Some(ar), Some(br)) = (a.ram_kib, b.ram_kib) else {
        return Err("Mémoire matérielle inconnue".into());
    };
    if ar.abs_diff(br) as f64 / ar.max(br) as f64 > 0.01 {
        return Err(
            "Capacités RAM différentes (tolérance de 1 % pour la mémoire réservée au noyau)."
                .into(),
        );
    }
    if a.engine_sha256 != b.engine_sha256 || a.helper_sha256 != b.helper_sha256 {
        return Err("Versions binaires différentes : utilisez exactement la même AppImage sur les deux distributions.".into());
    }
    Ok(())
}
fn uncertain(a: &Metric, b: &Metric) -> bool {
    !a.confirmation_reasons.is_empty()
        || !b.confirmation_reasons.is_empty()
        || (b.mean() / a.mean()).ln().abs() <= a.cv() + b.cv()
        || a.work_description != b.work_description
}
pub fn compare(a: &Session, b: &Session) -> Result<Comparison, String> {
    if a.system.is_some() || b.system.is_some() {
        return Err("Utiliser le comparateur système v2 pour ces rapports".into());
    }
    a.validate()?;
    b.validate()?;
    hardware(a, b)?;
    let aa: BTreeMap<_, _> = a.metrics.iter().map(|m| (&m.id, m)).collect();
    let bb: BTreeMap<_, _> = b.metrics.iter().map(|m| (&m.id, m)).collect();
    if aa.keys().ne(bb.keys()) {
        return Err("Ensembles de tests différents : comparaison refusée.".into());
    }
    let mut differences = Vec::new();
    for (id, reference) in aa {
        let candidate = bb[id];
        if reference.category != candidate.category || reference.unit != candidate.unit {
            return Err("Unités ou catégories incompatibles".into());
        }
        let index = 100. * (candidate.mean() / reference.mean());
        if !index.is_finite() || index <= 0. {
            return Err("Rapport de performances hors limites".into());
        }
        differences.push(Difference {
            id: id.clone(),
            label: reference.label.clone(),
            category: reference.category.clone(),
            candidate_index: index,
            reference_rate: reference.mean(),
            candidate_rate: candidate.mean(),
            unit: reference.unit.clone(),
            reference_cv: reference.cv(),
            candidate_cv: candidate.cv(),
            confirmation_required: uncertain(reference, candidate),
        });
    }
    let mut categories = Vec::new();
    for (id, label) in [
        ("cpu", "Processeur"),
        ("cache", "Influence du cache mémoire"),
        ("glibc", "Bibliothèque glibc"),
    ] {
        let members: Vec<_> = differences.iter().filter(|d| d.category == id).collect();
        if members.is_empty() {
            return Err(format!(
                "Catégorie {id} absente : aucun zéro ni remplacement automatique."
            ));
        }
        categories.push(Category {
            id: id.into(),
            label: label.into(),
            candidate_index: geo(members.iter().map(|d| d.candidate_index)),
            confirmation_required: members.iter().any(|d| d.confirmation_required),
        });
    }
    let native_partial_index = geo(categories[..2].iter().map(|c| c.candidate_index));
    let application_index = categories[2].candidate_index;
    let synthesis_index = geo([native_partial_index, application_index].into_iter());
    let mut software_differences = Vec::new();
    for (name, left, right) in [
        (
            "Noyau",
            a.fingerprint.kernel.clone(),
            b.fingerprint.kernel.clone(),
        ),
        (
            "Microcode",
            a.fingerprint.microcode.clone(),
            b.fingerprint.microcode.clone(),
        ),
        (
            "glibc",
            a.glibc_raw["glibc_version"].as_str().map(str::to_owned),
            b.glibc_raw["glibc_version"].as_str().map(str::to_owned),
        ),
    ] {
        software_differences.push(format!(
            "{name} : {} → {}",
            left.unwrap_or_else(|| "Indisponible".into()),
            right.unwrap_or_else(|| "Indisponible".into())
        ));
    }
    software_differences.push(format!(
        "Gouverneurs CPU : {:?} → {:?}",
        a.fingerprint.governors, b.fingerprint.governors
    ));
    software_differences.push(format!(
        "Préférences énergétiques : {:?} → {:?}",
        a.fingerprint.energy_preferences, b.fingerprint.energy_preferences
    ));
    let confirmation_required = categories.iter().any(|c| c.confirmation_required);
    Ok(Comparison{score_version:SCORE_VERSION.into(),reference:a.fingerprint.distribution.clone(),candidate:b.fingerprint.distribution.clone(),categories,native_partial_index,application_index,synthesis_index,confirmation_required,differences,software_differences,warnings:vec!["Synthèse CPU/cache/glibc uniquement : aucun score GPU, RAM dédiée ou charge combinée.".into(),"Un écart inférieur à la somme des variations observées, une mesure incertaine ou une calibration différente demande confirmation. Ce critère n’est pas un test de significativité statistique.".into(),"Les écarts ne sont pas attribués automatiquement au noyau ou à glibc.".into()]})
}
impl Comparison {
    pub fn details_text(&self) -> String {
        self.differences.iter().map(|d|format!("{}\nA : {:.2} {} · variation {:.2} %\nB : {:.2} {} · variation {:.2} %\nIndice B : {:.2}{}",d.label,d.reference_rate,d.unit,d.reference_cv*100.,d.candidate_rate,d.unit,d.candidate_cv*100.,d.candidate_index,if d.confirmation_required{" · Écart à confirmer"}else{""})).collect::<Vec<_>>().join("\n\n")
    }
    pub fn text(&self) -> String {
        let winner = |index: f64, confirm: bool| {
            if (index - 100.).abs() < 1e-9 {
                return "Égalité : aucun écart mesuré".into();
            }
            let (name, percent) = if index >= 100. {
                (&self.candidate, index - 100.)
            } else {
                (&self.reference, (100. / index - 1.) * 100.)
            };
            format!(
                "{name} en tête de {percent:.1} %{}",
                if confirm {
                    " · Écart à confirmer"
                } else {
                    ""
                }
            )
        };
        let mut out=format!("{} contre {}\nRéférence : indice 100\n\nSynthèse CPU/cache/glibc : {:.1}\n{}\n\nIndice natif partiel (CPU/cache) : {:.1}\nIndice applicatif (glibc) : {:.1}\n",self.reference,self.candidate,self.synthesis_index,winner(self.synthesis_index,self.confirmation_required),self.native_partial_index,self.application_index);
        for c in &self.categories {
            out.push_str(&format!(
                "\n{} : {:.1} · {}",
                c.label,
                c.candidate_index,
                winner(c.candidate_index, c.confirmation_required)
            ));
        }
        out.push_str("\n\nVersions et réglages\n");
        out.push_str(&self.software_differences.join("\n"));
        out.push_str("\n\n");
        out.push_str(&self.warnings.join("\n"));
        out
    }
}
