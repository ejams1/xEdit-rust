{ parity probe: a-10 }
unit P_a_10;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  a: array[0..2] of Integer;
begin
  a['1'] := 4;
  Show('a[1]', a[1]);
  Show('a[''1'']', a['1']);
end;
end.
