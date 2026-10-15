{ parity probe: v-slot-51 }
unit P_v_slot_51;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
begin
  v := VarType(Now);
  Show('VarType(Now)', v);
end;
end.
