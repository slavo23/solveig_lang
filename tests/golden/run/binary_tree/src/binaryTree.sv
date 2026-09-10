data Tree a = Leaf | Node (Tree a) a (Tree a)
;

singleton : forall a. a -> Tree a
;
singleton x = Node Leaf x Leaf
;

size : forall a. Tree a -> Int
;
size t =
    case t of
        Leaf => 0
        | Node l _ r => 1 + size l + size r ;

height : forall a. Tree a -> Int
;
height t =
    case t of
        Leaf => 0
        | Node l _ r =>
            let hl = height l in let hr = height r in if hl > hr then hl + 1 else hr + 1 ;

mapTree : forall a b. (a -> b) -> Tree a -> Tree b
;
mapTree f t =
    case t of
        Leaf => Leaf
        | Node l x r => Node (mapTree f l) (f x) (mapTree f r) ;

foldTree : forall a b. b -> (b -> a -> b -> b) -> Tree a -> b
;
foldTree z f t =
    case t of
        Leaf => z
        | Node l x r => f (foldTree z f l) x (foldTree z f r) ;

mirror : forall a. Tree a -> Tree a
;
mirror t =
    case t of
        Leaf => Leaf
        | Node l x r => Node (mirror r) x (mirror l) ;

inorder : forall a. Tree a -> [a]
;
inorder t =
    case t of
        Leaf => []
        | Node l x r => append (inorder l) (x :: inorder r) ;

preorder : forall a. Tree a -> [a]
;
preorder t =
    case t of
        Leaf => []
        | Node l x r => x :: append (preorder l) (preorder r) ;

postorder : forall a. Tree a -> [a]
;
postorder t =
    case t of
        Leaf => []
        | Node l x r => append (postorder l) (append (postorder r) [x]) ;

sumTree : Tree Int -> Int
;
sumTree t =
    case t of
        Leaf => 0
        | Node l x r => sumTree l + x + sumTree r ;

containsInt : Int -> Tree Int -> Bool
;
containsInt target t =
    case t of
        Leaf => False
        | Node l x r =>
            if target == x
                then True
                else if target < x then containsInt target l else containsInt target r ;

insertInt : Int -> Tree Int -> Tree Int
;
insertInt value t =
    case t of
        Leaf => singleton value
        | Node l x r =>
            if value < x
                then Node (insertInt value l) x r
                else if value > x then Node l x (insertInt value r) else t ;

fromListInt : [Int] -> Tree Int
;
fromListInt xs =
    case xs of
        [] => Leaf
        | y :: ys => insertInt y (fromListInt ys) ;

countLeaves : forall a. Tree a -> Int
;
countLeaves t =
    case t of
        Leaf => 1
        | Node l _ r => countLeaves l + countLeaves r ;

sampleTree : Tree Int
;
sampleTree = fromListInt [8, 3, 10, 1, 6, 14, 4, 7, 13, 17, 40, 2]
;

sampleSize : Int
;
sampleSize = size sampleTree
;

sampleHeight : Int
;
sampleHeight = height sampleTree
;

sampleInorder : [Int]
;
sampleInorder = inorder sampleTree
;

sampleHas7 : Bool
;
sampleHas7 = containsInt 7 sampleTree
;

sampleHas9 : Bool
;
sampleHas9 = containsInt 9 sampleTree
;
