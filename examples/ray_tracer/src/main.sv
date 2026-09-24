import std.io(BufferMode(..), IOMode(..), hPutStr, hSetBuffering, withFile)
import std.math(fromInt, sqrt, square, truncateToInt)
import std.text(TextBuilder, buildText, emptyTextBuilder, textBuilder)

data Vec3 = Vec3 Float Float Float

data Ray = Ray Vec3 Vec3

data Material = Material Vec3 Float Float

data Sphere = Sphere Vec3 Float Material

data Hit = Hit Vec3 Vec3 Material

data Camera = Camera Vec3 Vec3 Vec3 Vec3

data AreaLight = AreaLight Vec3 Float Vec3 Float

data World = World [Sphere] AreaLight

addVec : Vec3 -> Vec3 -> Vec3
addVec (Vec3 ax ay az) (Vec3 bx by bz) = Vec3 (ax + bx) (ay + by) (az + bz)

subtractVec : Vec3 -> Vec3 -> Vec3
subtractVec (Vec3 ax ay az) (Vec3 bx by bz) = Vec3 (ax - bx) (ay - by) (az - bz)

multiplyVec : Vec3 -> Vec3 -> Vec3
multiplyVec (Vec3 ax ay az) (Vec3 bx by bz) = Vec3 (ax * bx) (ay * by) (az * bz)

scaleVec : Float -> Vec3 -> Vec3
scaleVec amount (Vec3 x y z) = Vec3 (amount * x) (amount * y) (amount * z)

negateVec : Vec3 -> Vec3
negateVec = scaleVec (-1.0)

dotVec : Vec3 -> Vec3 -> Float
dotVec (Vec3 ax ay az) (Vec3 bx by bz) = ax * bx + ay * by + az * bz

dotSelf : Vec3 -> Float
dotSelf vector = dotVec vector vector

lengthVec : Vec3 -> Float
lengthVec = sqrt . dotSelf

normalise : Vec3 -> Vec3
normalise vector =
    let magnitude =
        lengthVec vector
    in if magnitude <= 0.0000001 then Vec3 0.0 0.0 0.0 else scaleVec (1.0 / magnitude) vector

reflectVec : Vec3 -> Vec3 -> Vec3
reflectVec direction normal =
    subtractVec direction (scaleVec (2.0 * dotVec direction normal) normal)

blendVec : Float -> Vec3 -> Vec3 -> Vec3
blendVec amount first second = addVec (scaleVec (1.0 - amount) first) (scaleVec amount second)

rayAt : Ray -> Float -> Vec3
rayAt (Ray origin direction) distance = addVec origin (scaleVec distance direction)

cameraFor : Int -> Int -> Camera
cameraFor width height =
    let aspectRatio =
        fromInt width / fromInt height
    in let origin =
        Vec3 0.0 0.25 1.1
    in let horizontal =
        Vec3 (1.7 * aspectRatio) 0.0 0.0
    in let vertical =
        Vec3 0.0 1.7 0.0
    in let lowerLeft =
        subtractVec
            (subtractVec (subtractVec origin (scaleVec 0.5 horizontal)) (scaleVec 0.5 vertical))
            (Vec3 0.0 0.0 1.2)
    in Camera origin lowerLeft horizontal vertical

cameraRay : Camera -> Float -> Float -> Ray
cameraRay (Camera origin lowerLeft horizontal vertical) u v =
    Ray
        origin
        (subtractVec
            (addVec lowerLeft (addVec (scaleVec u horizontal) (scaleVec v vertical)))
            origin)

intersectionDistance : Ray -> Float -> Float -> Vec3 -> Float -> Maybe Float
intersectionDistance (Ray origin direction) minimum maximum center radius =
    let offset =
        subtractVec origin center
    in let a =
        dotSelf direction
    in let halfB =
        dotVec offset direction
    in let c =
        dotSelf offset - radius * radius
    in let discriminant =
        halfB * halfB - a * c
    in if discriminant < 0.0
        then Nothing
        else
            let rootMagnitude =
                sqrt discriminant
            in let nearRoot =
                (-halfB - rootMagnitude) / a
            in if inRange minimum maximum nearRoot
                then Just nearRoot
                else
                    let farRoot =
                        (-halfB + rootMagnitude) / a
                    in if inRange minimum maximum farRoot then Just farRoot else Nothing

inRange : Float -> Float -> Float -> Bool
inRange minimum maximum value = and (value >= minimum) (value <= maximum)

sphereHit : Ray -> Vec3 -> Float -> Material -> Float -> Hit
sphereHit ray center radius material distance =
    let point =
        rayAt ray distance
    in let normal = scaleVec (1.0 / radius) (subtractVec point center) in Hit point normal material

nearestHit : Ray -> Float -> Float -> [Sphere] -> Maybe Hit
nearestHit ray minimum maximum spheres = nearestHitFrom ray minimum maximum spheres Nothing

nearestHitFrom : Ray -> Float -> Float -> [Sphere] -> Maybe Hit -> Maybe Hit
nearestHitFrom _ _ _ [] best = best
nearestHitFrom ray minimum closest (Sphere center radius material :: rest) best =
    case intersectionDistance ray minimum closest center radius of
        Nothing => nearestHitFrom ray minimum closest rest best
        | Just distance =>
            nearestHitFrom
                ray
                minimum
                distance
                rest
                (Just (sphereHit ray center radius material distance))

occluded : Ray -> Float -> [Sphere] -> Bool
occluded _ _ [] = False
occluded ray maximum (Sphere center radius _ :: rest) =
    case intersectionDistance ray 0.002 maximum center radius of
        Nothing => occluded ray maximum rest
        | Just _ => True

lightSamples : [Vec3]
lightSamples =
    [Vec3 (-0.75) 0.0 (-0.25), Vec3 0.25 0.0 (-0.75), Vec3 0.75 0.0 0.25, Vec3 (-0.25) 0.0 0.75]

pow32 : Float -> Float
pow32 = square . square . square . square . square

lightSample : [Sphere] -> AreaLight -> Vec3 -> Vec3 -> Vec3 -> Vec3 -> Vec3 -> Vec3 -> Float -> Vec3
lightSample
    spheres
    (AreaLight position radius lightColor power)
    point
    normal
    viewDirection
    albedo
    shadowOrigin
    sample
    specularStrength =
    let samplePosition =
        addVec position (scaleVec radius sample)
    in let towardLight =
        subtractVec samplePosition point
    in let distanceSquared =
        dotSelf towardLight
    in let distance =
        sqrt distanceSquared
    in let lightDirection =
        scaleVec (1.0 / distance) towardLight
    in let diffuse =
        maxFloat 0.0 (dotVec normal lightDirection)
    in if diffuse <= 0.0
        then black
        else if occluded (Ray shadowOrigin lightDirection) (distance - 0.004) spheres
                then black
                else
                    let attenuation =
                        power / (1.0 + 0.12 * distanceSquared)
                    in let halfDirection =
                        normalise (addVec lightDirection viewDirection)
                    in let specular =
                        specularStrength * pow32 (maxFloat 0.0 (dotVec normal halfDirection))
                    in addVec
                        (scaleVec (attenuation * diffuse) (multiplyVec albedo lightColor))
                        (scaleVec (attenuation * specular) lightColor)

areaLighting : [Sphere] -> AreaLight -> Vec3 -> Vec3 -> Vec3 -> Vec3 -> Float -> Vec3
areaLighting spheres light point normal viewDirection albedo specularStrength =
    let shadowOrigin =
        addVec point (scaleVec 0.002 normal)
    in scaleVec
        0.25
        (areaLightingFrom
            spheres
            light
            point
            normal
            viewDirection
            albedo
            shadowOrigin
            specularStrength
            lightSamples
            black)

areaLightingFrom :
    [Sphere] -> AreaLight -> Vec3 -> Vec3 -> Vec3 -> Vec3 -> Vec3 -> Float -> [Vec3] -> Vec3 -> Vec3
areaLightingFrom _ _ _ _ _ _ _ _ [] accumulated = accumulated
areaLightingFrom
    spheres
    light
    point
    normal
    viewDirection
    albedo
    shadowOrigin
    specularStrength
    (sample :: rest)
    accumulated =
    areaLightingFrom
        spheres
        light
        point
        normal
        viewDirection
        albedo
        shadowOrigin
        specularStrength
        rest
        (addVec
            accumulated
            (lightSample
                spheres
                light
                point
                normal
                viewDirection
                albedo
                shadowOrigin
                sample
                specularStrength))

ambientLight : Vec3
ambientLight = Vec3 0.055 0.065 0.09

black : Vec3
black = Vec3 0.0 0.0 0.0

localColor : [Sphere] -> AreaLight -> Vec3 -> Vec3 -> Vec3 -> Vec3 -> Float -> Vec3
localColor spheres light point normal viewDirection albedo specularStrength =
    addVec
        (multiplyVec albedo ambientLight)
        (areaLighting spheres light point normal viewDirection albedo specularStrength)

skyColor : Ray -> Vec3
skyColor (Ray _ direction) =
    let Vec3 _ y _ =
        normalise direction
    in let blend =
        0.5 * (y + 1.0)
    in addVec (scaleVec (1.0 - blend) (Vec3 1.0 1.0 1.0)) (scaleVec blend (Vec3 0.34 0.58 0.92))

traceRay : World -> Int -> Ray -> Vec3
traceRay (World spheres light) = traceRayIn spheres light

traceRayIn : [Sphere] -> AreaLight -> Int -> Ray -> Vec3
traceRayIn spheres light remainingBounces ray =
    case nearestHit ray 0.002 1000000.0 spheres of
        Nothing => skyColor ray
        | Just hit => shadeHit spheres light remainingBounces ray hit

shadeHit : [Sphere] -> AreaLight -> Int -> Ray -> Hit -> Vec3
shadeHit
    spheres
    light
    remainingBounces
    (Ray _ direction)
    (Hit point normal (Material albedo reflectivity specularStrength)) =
    let unitDirection =
        normalise direction
    in let viewDirection =
        negateVec unitDirection
    in let surface =
        localColor spheres light point normal viewDirection albedo specularStrength
    in if or (remainingBounces <= 0) (reflectivity <= 0.0)
        then surface
        else
            let reflectedDirection =
                reflectVec unitDirection normal
            in let reflectedOrigin =
                addVec point (scaleVec 0.002 normal)
            in let reflected =
                traceRayIn
                    spheres
                    light
                    (remainingBounces - 1)
                    (Ray reflectedOrigin reflectedDirection)
            in blendVec reflectivity surface reflected

maxFloat : Float -> Float -> Float
maxFloat left right = if left >= right then left else right

clamp01 : Float -> Float
clamp01 value = if value < 0.0 then 0.0 else if value > 1.0 then 1.0 else value

colorChannel : Float -> Int
colorChannel value =
    case truncateToInt (255.999 * sqrt (clamp01 value)) of
        Nothing => 0
        | Just channel => channel

pixelBuilder : Vec3 -> TextBuilder
pixelBuilder (Vec3 red green blue) =
    textBuilder (show (colorChannel red))
        <> textBuilder " "
        <> textBuilder (show (colorChannel green))
        <> textBuilder " "
        <> textBuilder (show (colorChannel blue))
        <> textBuilder "\n"

renderPixel : World -> Camera -> Int -> Int -> Int -> Int -> TextBuilder
renderPixel world camera width height x y =
    let u =
        fromInt x / fromInt (width - 1)
    in let v =
        fromInt (height - 1 - y) / fromInt (height - 1)
    in pixelBuilder . traceRay world 2 $ cameraRay camera u v

renderColumns : World -> Camera -> Int -> Int -> Int -> Int -> TextBuilder -> TextBuilder
renderColumns world camera width height x y output =
    if x >= width
        then output
        else
            renderColumns
                world
                camera
                width
                height
                (x + 1)
                y
                (output <> renderPixel world camera width height x y)

renderRow : World -> Camera -> Int -> Int -> Int -> String
renderRow world camera width height y =
    buildText (renderColumns world camera width height 0 y emptyTextBuilder)

ppmHeader : Int -> Int -> String
ppmHeader width height =
    buildText
        (textBuilder "P3\n"
            <> textBuilder (show width)
            <> textBuilder " "
            <> textBuilder (show height)
            <> textBuilder "\n255\n")

renderRowsTo : Handle -> World -> Camera -> Int -> Int -> Int -> Task ()
renderRowsTo output world camera width height y =
    if y >= height
        then pureIO ()
        else do
                hPutStr output (renderRow world camera width height y)
                renderRowsTo output world camera width height (y + 1)

writePpm : World -> Camera -> Int -> Int -> Handle -> Task ()
writePpm world camera width height output = do
    hSetBuffering output (BlockBuffering (Just 65536))
    hPutStr output (ppmHeader width height)
    renderRowsTo output world camera width height 0

world : World
world =
    World
        [
            Sphere (Vec3 0.0 0.0 (-1.0)) 0.5 (Material (Vec3 0.85 0.18 0.14) 0.08 0.35),
            Sphere (Vec3 (-1.05) 0.05 (-1.65)) 0.55 (Material (Vec3 0.1 0.3 0.88) 0.24 0.65),
            Sphere (Vec3 1.0 (-0.05) (-1.35)) 0.45 (Material (Vec3 0.94 0.58 0.08) 0.4 0.8),
            Sphere (Vec3 0.0 (-100.5) (-1.0)) 100.0 (Material (Vec3 0.28 0.64 0.26) 0.1 0.08)
        ]
        (AreaLight (Vec3 (-2.0) 3.5 0.75) 0.9 (Vec3 1.0 0.88 0.72) 3.2)

main : Task ()
main =
    let width =
        512
    in let height =
        512
    in let camera =
        cameraFor width height
    in do
        withFile "output/scene.ppm" WriteMode (writePpm world camera width height)
        printLine ("wrote output/scene.ppm (" <> show width <> "x" <> show height <> ")")
