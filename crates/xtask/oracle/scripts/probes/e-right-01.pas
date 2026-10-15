{ parity probe: e-right-01 }
unit P_e_right_01;

function Initialize: integer;
var
  x: Variant;
begin
  x := 1 - (1 - (1 - (1 - (1 - (1 - (1 - (1 - (1 - (1 - 1)))))))));
  AddMessage('x = ' + VarToStr(x));
end;
end.
