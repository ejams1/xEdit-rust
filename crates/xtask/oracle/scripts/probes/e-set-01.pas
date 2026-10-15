{ parity probe: e-set-01 }
unit P_e_set_01;

function Initialize: integer;
var
  x: Variant;
begin
  x := [1, 2] + [3];
  AddMessage('x = ' + IntToStr(x));
end;
end.
