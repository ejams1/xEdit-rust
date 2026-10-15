{ parity probe: a-04 }
unit P_a_04;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  a: array of string;
begin
  SetLength(a, 2);
  Show('a[0]', a[0]);
  a[1] := 'x';
  Show('a[1]', a[1]);
end;
end.
