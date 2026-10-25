# M10 proposal 4: Fill first, scripts after, detection last

Part of [M10: Forms](../README.md) · [all proposals](README.md)

**Fill first, scripts after, detection last.** The roadmap's order is
fill, scripts, authoring, detection, data, flatten. The corpus says
most forms need no scripts:
- None of the 29 IRS forms has a field script. Their only scripts are
  three Adobe version checks per file (proposal 13).
- Format scripts are needed just to display values in 10 of the 40
  files: every state form except Colorado. Only 3 calculate (GA, OH,
  VA), and only Georgia does real arithmetic.
- So: groundwork and the page-tool fixes (slices 1–5), appearances and
  filling (6–14), flatten and data (15–17), scripts (18–22),
  authoring (23–26), detection (27–28), then the plugin API and docs
  (29).
- Data comes before scripts because IRS forms need it and it doesn't
  need scripts. Once scripts land, an import recalculates.
