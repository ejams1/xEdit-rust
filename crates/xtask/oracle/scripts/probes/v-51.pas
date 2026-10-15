{ parity probe: v-51 }
unit P_v_51;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
begin
  v := '3' shl 1;
  Show('''3'' shl 1', v);
end;
end.
