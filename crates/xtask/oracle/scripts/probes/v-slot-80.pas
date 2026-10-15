{ parity probe: v-slot-80 }
unit P_v_slot_80;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
  e: IInterface;
begin
  v := VarType(e);
  Show('VarType(e)', v);
end;
end.
