{ parity probe: e-str-199 }
unit P_e_str_199;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  x: Variant;
begin
  x := 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a' + 'a';
  AddMessage('x = ' + VarToStr(x));
end;
end.
