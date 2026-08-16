# Preflight

Template: 12 in center label, A/B sheet
Output condition: FOGRA39
Target: CMYK at 300 DPI

| Status | Check | Detail |
| --- | --- | --- |
| pass | Artwork slot count matches template placements | 2 artwork slot(s) mapped to print placement(s). |
| pass | Artwork resolution meets target DPI | Lowest effective resolution is 300.5 ppi for a 300 ppi target. |
| pass | Final raster is converted through the exporter color pipeline | sRGB raster converted to target CMYK ICC with moxcms |
| pass | PDF embeds an ICC output intent | Embedded 4 channel ICCBased image space and OutputIntent FOGRA39. |
| pass | PDF header is present | Emitted file starts with %PDF-1.7. |
| pass | PDF boxes match template document size | MediaBox, TrimBox, and BleedBox are 126.000 x 232.000 mm. |
| pass | Embedded raster dimensions match target DPI | Embedded raster is 1489 x 2741 px at 300 ppi. |
| pass | PDF/X markers and XMP metadata are present | Emitted PDF declares pdf/x-4 in Info and XMP metadata. |
| pass | No RGB PDF color objects are present | The emitted PDF object text contains no DeviceRGB, CalRGB, or Lab color spaces. |
| pass | Template guides are not written to the final PDF layer | The final PDF contains one raster image XObject and no source guide object identifiers. |
| pass | Cutout guide areas are masked in the final raster | Template contains no explicit no-print cutout guides. |
| pass | Safety clearance was confirmed before export | 2 safety area(s) were confirmed clear before plant-ready export. |
