{ parity probe: x-330 }
unit P_x_330;

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
  v := nil is TStringList; AddMessage(VarToStr(v));
  AddMessage('after');
end;
end.
