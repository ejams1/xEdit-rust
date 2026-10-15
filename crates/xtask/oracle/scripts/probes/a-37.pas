{ parity probe: a-37 }
unit P_a_37;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  a: array[0..2] of TStringList;
begin
  a[0] := TStringList.Create;
  a[0].Add('x');
  Show('Count', a[0].Count);
  Show('a[1]', VarType(a[1]));
end;
end.
