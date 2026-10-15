{ parity probe: e-side-01 }
unit P_e_side_01;

var
  n: Integer;

function Tick: Integer;
begin
  Inc(n);
  if n mod 50 = 0 then AddMessage('tick ' + IntToStr(n));
  Result := 1;
end;

function Initialize: integer;
var
  x: Variant;
begin
  x := Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick + Tick;
  AddMessage('x = ' + VarToStr(x));
end;
end.
