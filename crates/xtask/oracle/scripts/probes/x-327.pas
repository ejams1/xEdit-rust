{ parity probe: x-327 }
unit P_x_327;

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
  v := sl is TObject; AddMessage(VarToStr(v));
  AddMessage('after');
end;
end.
