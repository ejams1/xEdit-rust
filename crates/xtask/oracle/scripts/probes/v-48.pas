{ parity probe: v-48 }
unit P_v_48;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
begin
  v := True xor 1;
  Show('True xor 1', v);
end;
end.
