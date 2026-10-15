{ parity probe: a-13 }
unit P_a_13;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  a: array[0..2] of Variant;
begin
  a[0] := 'x';
  Show('a[0]', a[0]);
  a[0] := 5;
  Show('a[0]', a[0]);
  Show('a[1]', a[1]);
end;
end.
