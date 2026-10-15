{ parity probe: r-06 }
unit P_r_06;

function Initialize: integer;
var
  p: TPoint;
begin
  p.X := 3;
  AddMessage('x ' + IntToStr(p.X));
end;
end.
