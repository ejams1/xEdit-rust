{ parity probe: x-160 }
unit P_x_160;

function Initialize: integer;
var
  i: Integer;
  d: Double;
  s: string;
  b: Boolean;
  sl: TStringList;
  v: Variant;
begin
  sl := TStringList.Create;
  sl.Add('a');
  v := 'abc'; v := v <> 4; AddMessage(VarToStr(v));
  AddMessage('after');
end;
end.
