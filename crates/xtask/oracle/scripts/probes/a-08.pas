{ parity probe: a-08 }
unit P_a_08;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  a: array[0..1, 0..1] of Integer;
begin
  a[1, 1] := 5;
  Show('a[1,1]', a[1, 1]);
  Show('a[0]', a[0]);
end;
end.
