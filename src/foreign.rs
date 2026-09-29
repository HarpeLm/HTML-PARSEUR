//! Tables pour le contenu "étranger" (SVG et MathML), spec §13.2.6.5.
//!
//! Le tokenizer met tous les noms en minuscules, mais SVG est sensible à la casse :
//! `<foreignobject>` doit redevenir `<foreignObject>`, `viewbox` redevenir `viewBox`.

use crate::dom::{AttrNamespace, Attribute};

/// Casse correcte d'un nom de balise SVG.
pub fn svg_tag_name(name: &str) -> Option<&'static str> {
    Some(match name {
        "altglyph" => "altGlyph",
        "altglyphdef" => "altGlyphDef",
        "altglyphitem" => "altGlyphItem",
        "animatecolor" => "animateColor",
        "animatemotion" => "animateMotion",
        "animatetransform" => "animateTransform",
        "clippath" => "clipPath",
        "feblend" => "feBlend",
        "fecolormatrix" => "feColorMatrix",
        "fecomponenttransfer" => "feComponentTransfer",
        "fecomposite" => "feComposite",
        "feconvolvematrix" => "feConvolveMatrix",
        "fediffuselighting" => "feDiffuseLighting",
        "fedisplacementmap" => "feDisplacementMap",
        "fedistantlight" => "feDistantLight",
        "fedropshadow" => "feDropShadow",
        "feflood" => "feFlood",
        "fefunca" => "feFuncA",
        "fefuncb" => "feFuncB",
        "fefuncg" => "feFuncG",
        "fefuncr" => "feFuncR",
        "fegaussianblur" => "feGaussianBlur",
        "feimage" => "feImage",
        "femerge" => "feMerge",
        "femergenode" => "feMergeNode",
        "femorphology" => "feMorphology",
        "feoffset" => "feOffset",
        "fepointlight" => "fePointLight",
        "fespecularlighting" => "feSpecularLighting",
        "fespotlight" => "feSpotLight",
        "fetile" => "feTile",
        "feturbulence" => "feTurbulence",
        "foreignobject" => "foreignObject",
        "glyphref" => "glyphRef",
        "lineargradient" => "linearGradient",
        "radialgradient" => "radialGradient",
        "textpath" => "textPath",
        _ => return None,
    })
}

/// Casse correcte d'un nom d'attribut SVG.
fn svg_attribute_name(name: &str) -> Option<&'static str> {
    Some(match name {
        "attributename" => "attributeName",
        "attributetype" => "attributeType",
        "basefrequency" => "baseFrequency",
        "baseprofile" => "baseProfile",
        "calcmode" => "calcMode",
        "clippathunits" => "clipPathUnits",
        "diffuseconstant" => "diffuseConstant",
        "edgemode" => "edgeMode",
        "filterunits" => "filterUnits",
        "glyphref" => "glyphRef",
        "gradienttransform" => "gradientTransform",
        "gradientunits" => "gradientUnits",
        "kernelmatrix" => "kernelMatrix",
        "kernelunitlength" => "kernelUnitLength",
        "keypoints" => "keyPoints",
        "keysplines" => "keySplines",
        "keytimes" => "keyTimes",
        "lengthadjust" => "lengthAdjust",
        "limitingconeangle" => "limitingConeAngle",
        "markerheight" => "markerHeight",
        "markerunits" => "markerUnits",
        "markerwidth" => "markerWidth",
        "maskcontentunits" => "maskContentUnits",
        "maskunits" => "maskUnits",
        "numoctaves" => "numOctaves",
        "pathlength" => "pathLength",
        "patterncontentunits" => "patternContentUnits",
        "patterntransform" => "patternTransform",
        "patternunits" => "patternUnits",
        "pointsatx" => "pointsAtX",
        "pointsaty" => "pointsAtY",
        "pointsatz" => "pointsAtZ",
        "preservealpha" => "preserveAlpha",
        "preserveaspectratio" => "preserveAspectRatio",
        "primitiveunits" => "primitiveUnits",
        "refx" => "refX",
        "refy" => "refY",
        "repeatcount" => "repeatCount",
        "repeatdur" => "repeatDur",
        "requiredextensions" => "requiredExtensions",
        "requiredfeatures" => "requiredFeatures",
        "specularconstant" => "specularConstant",
        "specularexponent" => "specularExponent",
        "spreadmethod" => "spreadMethod",
        "startoffset" => "startOffset",
        "stddeviation" => "stdDeviation",
        "stitchtiles" => "stitchTiles",
        "surfacescale" => "surfaceScale",
        "systemlanguage" => "systemLanguage",
        "tablevalues" => "tableValues",
        "targetx" => "targetX",
        "targety" => "targetY",
        "textlength" => "textLength",
        "viewbox" => "viewBox",
        "viewtarget" => "viewTarget",
        "xchannelselector" => "xChannelSelector",
        "ychannelselector" => "yChannelSelector",
        "zoomandpan" => "zoomAndPan",
        _ => return None,
    })
}

pub fn adjust_svg_attributes(attrs: &mut [Attribute]) {
    for attr in attrs {
        if let Some(fixed) = svg_attribute_name(&attr.name) {
            attr.name = fixed.to_string();
        }
    }
}

pub fn adjust_mathml_attributes(attrs: &mut [Attribute]) {
    for attr in attrs {
        if attr.name == "definitionurl" {
            attr.name = "definitionURL".to_string();
        }
    }
}

/// `xlink:href` devient l'attribut `href` de l'espace de noms XLink, etc.
pub fn adjust_foreign_attributes(attrs: &mut [Attribute]) {
    for attr in attrs {
        let (ns, local) = match attr.name.as_str() {
            "xlink:actuate" | "xlink:arcrole" | "xlink:href" | "xlink:role" | "xlink:show"
            | "xlink:title" | "xlink:type" => (AttrNamespace::XLink, &attr.name[6..]),
            "xml:lang" | "xml:space" => (AttrNamespace::Xml, &attr.name[4..]),
            "xmlns" => (AttrNamespace::Xmlns, "xmlns"),
            "xmlns:xlink" => (AttrNamespace::Xmlns, "xlink"),
            _ => continue,
        };
        attr.name = local.to_string();
        attr.ns = ns;
    }
}

/// Balises HTML qui font sortir du SVG/MathML (§13.2.6.5).
pub fn breaks_out_of_foreign(name: &str) -> bool {
    matches!(
        name,
        "b" | "big" | "blockquote" | "body" | "br" | "center" | "code" | "dd" | "div" | "dl" | "dt"
            | "em" | "embed" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "head" | "hr" | "i" | "img"
            | "li" | "listing" | "menu" | "meta" | "nobr" | "ol" | "p" | "pre" | "ruby" | "s"
            | "small" | "span" | "strong" | "strike" | "sub" | "sup" | "table" | "tt" | "u" | "ul"
            | "var"
    )
}
